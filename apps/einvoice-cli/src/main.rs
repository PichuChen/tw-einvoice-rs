#![forbid(unsafe_code)]

use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use tw_einvoice_client::{PreparedSubmission, SubmissionPlan, prepare_submission};
use tw_einvoice_core::MigVersion;
use tw_einvoice_envelope::{
    EnvelopeRouting, InvoiceEnvelope, InvoicePackMetadata, MigPayload, PartyInfo, RoutingInfo,
};
use tw_einvoice_signing::PfxSigner;
use tw_einvoice_transport::{
    filename::TurnkeyPackFilename,
    gateway::{GatewayCredentials, Pfs001HttpClient, ServiceType, ZipMode},
    inbound::ParsedReconciliation,
    native::{GatewayVersionProfile, NativeSubmitter},
    reconciliation::{ReconciledObject, ReconciliationPoller},
    sftp::{SftpConfig, SftpCredentials, Ssh2RemoteInbox, Ssh2SftpUploader},
};

const DEFAULT_PFX_PASSWORD_ENV: &str = "TW_EINVOICE_PFX_PASSWORD";
const DEFAULT_TRANSPORT_PASSWORD_ENV: &str = "TW_EINVOICE_TRANSPORT_PASSWORD";
const DEFAULT_API_VERSION: &str = "3.1.3";
const DEFAULT_TURNKEY_VERSION: &str = "3.2.1";

#[derive(Debug, Parser)]
#[command(
    name = "tw-einvoice",
    about = "Turnkey-compatible Taiwan e-Invoice prepare/submit/receive tooling",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate, pack, sign, and persist an artifact without network I/O.
    Prepare(PrepareArgs),
    /// Prepare and persist an artifact, then submit it through SFTP + PFS001.
    Submit(SubmitArgs),
    /// Durably receive and classify objects from the SFTP /out inbox.
    Receive(ReceiveArgs),
}

#[derive(Debug, Args)]
struct PrepareArgs {
    #[command(flatten)]
    common: CommonPrepareArgs,
}

#[derive(Debug, Args)]
struct SubmitArgs {
    #[command(flatten)]
    common: CommonPrepareArgs,

    /// Explicit platform environment. There is intentionally no implicit default.
    #[arg(long, value_enum)]
    environment: PlatformEnvironment,

    /// OpenSSH `known_hosts` file used for mandatory SFTP host-key verification.
    #[arg(long)]
    known_hosts: PathBuf,

    /// Ministry transport account/login ID used by both SFTP and PFS001.
    #[arg(long)]
    transport_id: String,

    /// Environment variable containing the transport password.
    #[arg(long, default_value = DEFAULT_TRANSPORT_PASSWORD_ENV)]
    transport_password_env: String,
}

#[derive(Debug, Args)]
struct ReceiveArgs {
    /// Explicit platform environment. There is intentionally no implicit default.
    #[arg(long, value_enum)]
    environment: PlatformEnvironment,

    /// OpenSSH `known_hosts` file used for mandatory SFTP host-key verification.
    #[arg(long)]
    known_hosts: PathBuf,

    /// Ministry transport account/login ID used for SFTP.
    #[arg(long)]
    transport_id: String,

    /// Environment variable containing the transport password.
    #[arg(long, default_value = DEFAULT_TRANSPORT_PASSWORD_ENV)]
    transport_password_env: String,

    /// Durable local directory for objects received from SFTP `/out`.
    #[arg(long)]
    inbox_dir: PathBuf,
}

#[derive(Debug, Args)]
struct CommonPrepareArgs {
    /// Full local Turnkey-compatible pack filename.
    #[arg(long)]
    filename: String,

    /// Canonical MIG XML payload(s), in the exact order to embed in `InvoicePack`.
    #[arg(long, required = true, num_args = 1..)]
    mig: Vec<PathBuf>,

    /// `FromVAC` `RoutingId`.
    #[arg(long)]
    from_vac: String,

    /// `ToVAC` `RoutingId`.
    #[arg(long)]
    to_vac: String,

    /// Optional From `PartyInfo` description.
    #[arg(long)]
    from_description: Option<String>,

    /// Optional To `PartyInfo` description.
    #[arg(long)]
    to_description: Option<String>,

    /// Optional `FromVAC` description.
    #[arg(long)]
    from_vac_description: Option<String>,

    /// Optional `ToVAC` description.
    #[arg(long)]
    to_vac_description: Option<String>,

    /// PFS001 action value associated with this message/task.
    #[arg(long)]
    action: String,

    /// Gateway service mode.
    #[arg(long, value_enum)]
    service: ServiceArg,

    /// Turnkey ZIP behavior.
    #[arg(long, value_enum, default_value = "plain")]
    zip: ZipArg,

    /// PKCS#12/PFX software certificate.
    #[arg(long)]
    pfx: PathBuf,

    /// Environment variable containing the PFX password.
    #[arg(long, default_value = DEFAULT_PFX_PASSWORD_ENV)]
    pfx_password_env: String,

    /// Durable signed-artifact destination. Written before any network I/O.
    #[arg(long)]
    output: PathBuf,

    /// Explicitly allow replacing an existing artifact file.
    #[arg(long)]
    force_output: bool,

    /// PFS gateway API compatibility version reported to the platform.
    #[arg(long, default_value = DEFAULT_API_VERSION)]
    api_version: String,

    /// Turnkey compatibility version reported to the platform.
    #[arg(long, default_value = DEFAULT_TURNKEY_VERSION)]
    turnkey_version: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ServiceArg {
    Store,
    Exchange,
}

impl ServiceArg {
    const fn into_service_type(self) -> ServiceType {
        match self {
            Self::Store => ServiceType::Store,
            Self::Exchange => ServiceType::Exchange,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ZipArg {
    Plain,
    Zip,
}

impl ZipArg {
    const fn into_zip_mode(self) -> ZipMode {
        match self {
            Self::Plain => ZipMode::Plain,
            Self::Zip => ZipMode::Zip,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlatformEnvironment {
    Test,
    Production,
}

impl PlatformEnvironment {
    const fn endpoints(self) -> PlatformEndpoints {
        match self {
            Self::Test => PlatformEndpoints {
                sftp_host: "tsftp.einvoice.nat.gov.tw",
                gateway_base: "https://tgw.einvoice.nat.gov.tw/gateway/api",
            },
            Self::Production => PlatformEndpoints {
                sftp_host: "sftp.einvoice.nat.gov.tw",
                gateway_base: "https://gw.einvoice.nat.gov.tw/gateway/api",
            },
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PlatformEndpoints {
    sftp_host: &'static str,
    gateway_base: &'static str,
}

#[derive(Debug)]
enum CliError {
    MissingSecretEnvironment { name: String },
    NonUnicodeSecretEnvironment { name: String },
    EmptySecretEnvironmentName,
    InboundObjectFailures { count: usize },
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSecretEnvironment { name } => {
                write!(
                    f,
                    "required secret environment variable {name:?} is not set"
                )
            }
            Self::NonUnicodeSecretEnvironment { name } => write!(
                f,
                "secret environment variable {name:?} is not valid Unicode"
            ),
            Self::EmptySecretEnvironmentName => {
                f.write_str("secret environment variable name must not be empty")
            }
            Self::InboundObjectFailures { count } => {
                write!(
                    f,
                    "{count} inbound object(s) failed durable receive or parsing"
                )
            }
        }
    }
}

impl Error for CliError {}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Command::Prepare(args) => run_prepare(&args),
        Command::Submit(args) => run_submit(&args),
        Command::Receive(args) => run_receive(&args),
    }
}

fn run_prepare(args: &PrepareArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(&args.common)?;
    persist_artifact(
        &args.common.output,
        prepared.artifact().as_upload_bytes(),
        args.common.force_output,
    )?;
    print_prepared_summary(&prepared, &args.common.output);
    Ok(())
}

fn run_submit(args: &SubmitArgs) -> Result<(), Box<dyn Error>> {
    let prepared = prepare(&args.common)?;

    // Durability boundary: no SFTP or HTTP client is invoked before the exact
    // signed artifact has been written and fsync'd successfully.
    persist_artifact(
        &args.common.output,
        prepared.artifact().as_upload_bytes(),
        args.common.force_output,
    )?;
    print_prepared_summary(&prepared, &args.common.output);

    let transport_password = read_secret_environment(&args.transport_password_env)?;
    let endpoints = args.environment.endpoints();

    let sftp_config = SftpConfig::new(endpoints.sftp_host, &args.known_hosts);
    let uploader = Ssh2SftpUploader::new(
        sftp_config,
        SftpCredentials::new(&args.transport_id, transport_password.clone()),
    );
    let notifier = Pfs001HttpClient::new(
        endpoints.gateway_base,
        GatewayCredentials::new(&args.transport_id, transport_password),
    )?;
    let submitter = NativeSubmitter::new(uploader, notifier);

    eprintln!(
        "submitting persisted artifact to {} environment ({})",
        args.environment.label(),
        prepared.filename().remote_filename()
    );
    let receipt = prepared.submit(&submitter)?;
    println!(
        "submitted remote_filename={} pfs001_status={}",
        receipt.remote_filename, receipt.gateway_status.status_code
    );
    Ok(())
}

fn run_receive(args: &ReceiveArgs) -> Result<(), Box<dyn Error>> {
    let transport_password = read_secret_environment(&args.transport_password_env)?;
    let endpoints = args.environment.endpoints();
    let remote = Ssh2RemoteInbox::new(
        SftpConfig::new(endpoints.sftp_host, &args.known_hosts),
        SftpCredentials::new(&args.transport_id, transport_password),
    );
    let mut poller = ReconciliationPoller::new(remote, &args.inbox_dir)?;
    let attempts = poller.poll_once()?;

    if attempts.is_empty() {
        println!(
            "received environment={} objects=0 inbox={}",
            args.environment.label(),
            args.inbox_dir.display()
        );
        return Ok(());
    }

    let mut failures = 0_usize;
    for attempt in attempts {
        let remote_name = attempt.object.name;
        match attempt.result {
            Ok(reconciled) => print_reconciled_object(&reconciled),
            Err(error) => {
                failures += 1;
                eprintln!("receive_error object={remote_name} error={error}");
            }
        }
    }

    if failures == 0 {
        Ok(())
    } else {
        Err(Box::new(CliError::InboundObjectFailures {
            count: failures,
        }))
    }
}

fn print_reconciled_object(reconciled: &ReconciledObject) {
    let path = reconciled.record.path().display();
    match &reconciled.record.reconciliation {
        Some(ParsedReconciliation::Process(result)) => {
            let codes = result
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "received kind=ProcessResult path={path} correlation_id={} diagnostics={} codes={codes}",
                result.message.id,
                result.diagnostics.len()
            );
        }
        Some(ParsedReconciliation::Summary(result)) => {
            for message in &result.messages {
                let issues = message.validation_issues(None);
                println!(
                    "received kind=SummaryResult path={path} correlation_id={} total={} good={} failed={} validation_issues={}",
                    message.info.id,
                    message.result.total.count,
                    message.result.good.count,
                    message.result.failed.count,
                    issues.len()
                );
            }
        }
        None => {
            println!(
                "received kind={:?} path={path} correlation_ids=0",
                reconciled.record.kind
            );
        }
    }
}

fn prepare(args: &CommonPrepareArgs) -> Result<PreparedSubmission, Box<dyn Error>> {
    let pfx_der = read_file(&args.pfx, "PFX identity")?;
    let pfx_password = read_secret_environment(&args.pfx_password_env)?;
    let signer = PfxSigner::from_der(&pfx_der, &pfx_password)?;
    let plan = build_submission_plan(args)?;
    prepare_submission(plan, &signer).map_err(Into::into)
}

fn build_submission_plan(args: &CommonPrepareArgs) -> Result<SubmissionPlan, Box<dyn Error>> {
    let filename = TurnkeyPackFilename::parse(&args.filename)?;
    let mut payloads = Vec::with_capacity(args.mig.len());
    for path in &args.mig {
        payloads.push(MigPayload::new(read_file(path, "MIG XML payload")?)?);
    }

    let envelope = InvoiceEnvelope::new(
        EnvelopeRouting {
            from: PartyInfo {
                identifier: filename.from_party.clone(),
                description: args.from_description.clone(),
            },
            from_vac: RoutingInfo {
                identifier: args.from_vac.clone(),
                description: args.from_vac_description.clone(),
            },
            to: PartyInfo {
                identifier: filename.to_party.clone(),
                description: args.to_description.clone(),
            },
            to_vac: RoutingInfo {
                identifier: args.to_vac.clone(),
                description: args.to_vac_description.clone(),
            },
        },
        InvoicePackMetadata {
            message_type: filename.common_name.message_type.clone(),
            version: MigVersion::V4_1,
        },
        payloads,
    )?;

    Ok(SubmissionPlan::new(
        envelope,
        filename,
        args.service.into_service_type(),
        args.zip.into_zip_mode(),
        args.action.clone(),
        GatewayVersionProfile::new(&args.api_version, &args.turnkey_version),
    ))
}

fn read_secret_environment(name: &str) -> Result<String, CliError> {
    if name.is_empty() {
        return Err(CliError::EmptySecretEnvironmentName);
    }

    match env::var(name) {
        Ok(secret) => Ok(secret),
        Err(env::VarError::NotPresent) => Err(CliError::MissingSecretEnvironment {
            name: name.to_owned(),
        }),
        Err(env::VarError::NotUnicode(_)) => Err(CliError::NonUnicodeSecretEnvironment {
            name: name.to_owned(),
        }),
    }
}

fn read_file(path: &Path, description: &str) -> io::Result<Vec<u8>> {
    fs::read(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to read {description} {}: {error}", path.display()),
        )
    })
}

fn persist_artifact(path: &Path, bytes: &[u8], force: bool) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true);

    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }

    let mut file = options.open(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to create artifact {}: {error}", path.display()),
        )
    })?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn print_prepared_summary(prepared: &PreparedSubmission, output: &Path) {
    println!(
        "prepared artifact={} local_filename={} remote_filename={} bytes={} signature={:?}",
        output.display(),
        prepared.filename().render(),
        prepared.filename().remote_filename(),
        prepared.artifact().turnkey_reported_size(),
        prepared.artifact().signature_algorithm()
    );
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use clap::Parser;

    use super::*;

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    fn temp_path(suffix: &str) -> PathBuf {
        let id = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        env::temp_dir().join(format!(
            "tw-einvoice-cli-test-{}-{id}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn submit_requires_explicit_environment() {
        let result = Cli::try_parse_from([
            "tw-einvoice",
            "submit",
            "--filename",
            "source.xml_12345678_PLATFORM_4.1-F0401-20260825-170000000-550e8400-e29b-41d4-a716-446655440000_1",
            "--mig",
            "invoice.xml",
            "--from-vac",
            "FROM",
            "--to-vac",
            "TO",
            "--action",
            "C0401",
            "--service",
            "store",
            "--pfx",
            "identity.pfx",
            "--output",
            "signed.cms",
            "--known-hosts",
            "known_hosts",
            "--transport-id",
            "transport-id",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn receive_requires_explicit_environment() {
        let result = Cli::try_parse_from([
            "tw-einvoice",
            "receive",
            "--known-hosts",
            "known_hosts",
            "--transport-id",
            "transport-id",
            "--inbox-dir",
            "inbox",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn prepare_never_accepts_literal_password_arguments() {
        let result = Cli::try_parse_from([
            "tw-einvoice",
            "prepare",
            "--filename",
            "source.xml_12345678_PLATFORM_4.1-F0401-20260825-170000000-550e8400-e29b-41d4-a716-446655440000_1",
            "--mig",
            "invoice.xml",
            "--from-vac",
            "FROM",
            "--to-vac",
            "TO",
            "--action",
            "C0401",
            "--service",
            "store",
            "--pfx",
            "identity.pfx",
            "--output",
            "signed.cms",
            "--pfx-password",
            "should-not-be-accepted",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn endpoints_are_environment_specific() {
        assert_eq!(
            PlatformEnvironment::Test.endpoints().sftp_host,
            "tsftp.einvoice.nat.gov.tw"
        );
        assert_eq!(
            PlatformEnvironment::Production.endpoints().gateway_base,
            "https://gw.einvoice.nat.gov.tw/gateway/api"
        );
    }

    #[test]
    fn persistence_refuses_overwrite_without_force() {
        let path = temp_path("artifact");
        persist_artifact(&path, b"first", false).unwrap();
        let error = persist_artifact(&path, b"second", false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"first");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn persistence_can_explicitly_replace_artifact() {
        let path = temp_path("artifact-force");
        persist_artifact(&path, b"first", false).unwrap();
        persist_artifact(&path, b"second", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn secret_variable_name_is_validated_without_exposing_values() {
        assert!(matches!(
            read_secret_environment(""),
            Err(CliError::EmptySecretEnvironmentName)
        ));

        let missing = format!(
            "TW_EINVOICE_TEST_MISSING_{}",
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        );
        let error = read_secret_environment(&missing).unwrap_err();
        assert!(error.to_string().contains(&missing));
    }
}
