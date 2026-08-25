use crate::{
    receiver::RemoteObject,
    reconciliation::RemoteInboxLister,
    sftp::{SftpInboxError, Ssh2RemoteInbox},
};

impl RemoteInboxLister for Ssh2RemoteInbox {
    fn list(&self) -> Result<Vec<RemoteObject>, SftpInboxError> {
        Ssh2RemoteInbox::list(self)
    }
}
