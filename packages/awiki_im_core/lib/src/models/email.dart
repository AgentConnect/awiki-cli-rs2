class EmailAttribute {
  const EmailAttribute({required this.key, required this.value});

  final String key;
  final String value;
}

class EmailAccount {
  const EmailAccount({
    this.mailboxAddress,
    this.displayName,
    this.status,
    this.attributes = const [],
  });

  final String? mailboxAddress;
  final String? displayName;
  final String? status;
  final List<EmailAttribute> attributes;
}

class EmailInboxQuery {
  const EmailInboxQuery({
    this.folder = 'inbox',
    this.limit = 20,
    this.offset = 0,
    this.unreadOnly = false,
  });

  final String folder;
  final int limit;
  final int offset;
  final bool unreadOnly;
}

class SendEmailRequest {
  const SendEmailRequest({
    required this.to,
    this.cc = const [],
    required this.subject,
    required this.bodyText,
    this.bodyHtml,
  });

  final List<String> to;
  final List<String> cc;
  final String subject;
  final String bodyText;
  final String? bodyHtml;
}

class EmailMessageSummary {
  const EmailMessageSummary({
    required this.id,
    this.folder,
    this.from = const [],
    this.to = const [],
    this.cc = const [],
    required this.subject,
    this.preview,
    this.receivedAt,
    this.sentAt,
    required this.unread,
    required this.hasAttachments,
    this.attachmentCount,
    this.attributes = const [],
  });

  final String id;
  final String? folder;
  final List<String> from;
  final List<String> to;
  final List<String> cc;
  final String subject;
  final String? preview;
  final String? receivedAt;
  final String? sentAt;
  final bool unread;
  final bool hasAttachments;
  final int? attachmentCount;
  final List<EmailAttribute> attributes;
}

class EmailMessageSummaryPage {
  const EmailMessageSummaryPage({
    required this.items,
    this.nextCursor,
    required this.hasMore,
  });

  final List<EmailMessageSummary> items;
  final String? nextCursor;
  final bool hasMore;
}

class EmailMessage {
  const EmailMessage({
    required this.summary,
    this.bodyText,
    this.bodyHtml,
    this.attachments = const [],
  });

  final EmailMessageSummary summary;
  final String? bodyText;
  final String? bodyHtml;
  final List<EmailAttachmentMetadata> attachments;
}

class EmailAttachmentMetadata {
  const EmailAttachmentMetadata({
    required this.index,
    this.filename,
    this.contentType,
    this.size,
  });

  final int index;
  final String? filename;
  final String? contentType;
  final int? size;
}

class EmailAttachmentContent {
  const EmailAttachmentContent({
    required this.messageId,
    required this.attachmentIndex,
    required this.filename,
    required this.contentType,
    this.size,
    required this.bytes,
  });

  final String messageId;
  final int attachmentIndex;
  final String filename;
  final String contentType;
  final int? size;
  final List<int> bytes;
}

class EmailMarkReadResult {
  const EmailMarkReadResult({required this.updated});

  final int updated;
}

class SendEmailResult {
  const SendEmailResult({
    required this.accepted,
    this.messageId,
    this.warnings = const [],
  });

  final bool accepted;
  final String? messageId;
  final List<String> warnings;
}

class EmailNotification {
  const EmailNotification({
    required this.id,
    this.mailboxAddress,
    this.fromAddr,
    required this.subject,
    this.preview,
    required this.hasAttachments,
    this.receivedAt,
    this.attributes = const [],
  });

  final String id;
  final String? mailboxAddress;
  final String? fromAddr;
  final String subject;
  final String? preview;
  final bool hasAttachments;
  final String? receivedAt;
  final List<EmailAttribute> attributes;
}

class EmailNotificationPage {
  const EmailNotificationPage({
    required this.items,
    this.nextCursor,
    required this.hasMore,
  });

  final List<EmailNotification> items;
  final String? nextCursor;
  final bool hasMore;
}
