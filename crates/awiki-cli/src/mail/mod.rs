mod service;
mod types;

pub use service::{notifications, notifications_plan, split_mail_list};
pub use types::{
    account_plan, attachment_download_plan, inbox_plan, mark_read_plan, read_plan, send_plan,
    CommandResult, MailError,
};
