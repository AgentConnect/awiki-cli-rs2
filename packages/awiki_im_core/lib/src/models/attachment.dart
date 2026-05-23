import 'message.dart';

sealed class AttachmentInput {
  const AttachmentInput();

  const factory AttachmentInput.localFile(String path) =
      LocalFileAttachmentInput;
  const factory AttachmentInput.bytes({
    String? filename,
    String? mimeType,
    required List<int> bytes,
  }) = BytesAttachmentInput;
}

class LocalFileAttachmentInput extends AttachmentInput {
  const LocalFileAttachmentInput(this.path);

  final String path;
}

class BytesAttachmentInput extends AttachmentInput {
  const BytesAttachmentInput({
    this.filename,
    this.mimeType,
    required this.bytes,
  });

  final String? filename;
  final String? mimeType;
  final List<int> bytes;
}

class AttachmentSendRequest {
  const AttachmentSendRequest({
    required this.target,
    required this.input,
    this.caption,
    this.mimeType,
    this.filename,
    this.idempotencyKey,
    this.waitForFinalAcceptance = false,
  });

  final MessageTarget target;
  final AttachmentInput input;
  final String? caption;
  final String? mimeType;
  final String? filename;
  final String? idempotencyKey;
  final bool waitForFinalAcceptance;
}

sealed class AttachmentDestination {
  const AttachmentDestination();

  const factory AttachmentDestination.localFile(String path) =
      LocalFileAttachmentDestination;
  const factory AttachmentDestination.memory() = MemoryAttachmentDestination;
}

class LocalFileAttachmentDestination extends AttachmentDestination {
  const LocalFileAttachmentDestination(this.path);

  final String path;
}

class MemoryAttachmentDestination extends AttachmentDestination {
  const MemoryAttachmentDestination();
}

class DownloadAttachmentRequest {
  const DownloadAttachmentRequest({
    required this.thread,
    required this.messageId,
    this.attachmentId,
    required this.destination,
    this.overwrite = false,
  });

  final ThreadRef thread;
  final String messageId;
  final String? attachmentId;
  final AttachmentDestination destination;
  final bool overwrite;
}

class DownloadedAttachment {
  const DownloadedAttachment({
    required this.attachmentId,
    this.filename,
    this.mimeType,
    this.sizeBytes,
    required this.destination,
    this.warnings = const [],
  });

  final String attachmentId;
  final String? filename;
  final String? mimeType;
  final int? sizeBytes;
  final DownloadedAttachmentDestination destination;
  final List<String> warnings;
}

sealed class DownloadedAttachmentDestination {
  const DownloadedAttachmentDestination();
}

class DownloadedAttachmentLocalFile extends DownloadedAttachmentDestination {
  const DownloadedAttachmentLocalFile(this.path);

  final String path;
}

class DownloadedAttachmentMemory extends DownloadedAttachmentDestination {
  const DownloadedAttachmentMemory(this.bytes);

  final List<int> bytes;
}
