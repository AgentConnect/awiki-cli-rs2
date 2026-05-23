use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttachmentSink {
    Memory,
    LocalFile { path: PathBuf, overwrite: bool },
}

pub(crate) fn attachment_destination_to_sink(
    destination: crate::attachments::AttachmentDestination,
    overwrite: bool,
) -> crate::ImResult<AttachmentSink> {
    match destination {
        crate::attachments::AttachmentDestination::Memory => Ok(AttachmentSink::Memory),
        crate::attachments::AttachmentDestination::LocalFile(path) => {
            Ok(AttachmentSink::LocalFile { path, overwrite })
        }
    }
}
