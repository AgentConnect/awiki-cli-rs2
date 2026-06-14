import 'dart:convert';

import 'error.dart';
import 'message_payload.dart';

const String messageMentionRangeUnitUnicodeCodePoint = 'unicode_code_point';

const Set<String> _forbiddenMentionFields = <String>{
  'sender',
  'sender_did',
  'from',
  'actor_did',
  'auth',
  'origin_proof',
  'proof',
  'signature',
};

enum MessageMentionSelector {
  all('all'),
  agents('agents'),
  humans('humans');

  const MessageMentionSelector(this.wireName);
  final String wireName;

  static MessageMentionSelector fromWireName(String value) {
    return switch (value) {
      'all' => MessageMentionSelector.all,
      'agents' => MessageMentionSelector.agents,
      'humans' => MessageMentionSelector.humans,
      _ => throw _invalidMention('unsupported mention selector'),
    };
  }
}

enum MessageMentionRole {
  addressee('addressee'),
  cc('cc');

  const MessageMentionRole(this.wireName);
  final String wireName;

  static MessageMentionRole fromWireName(String? value) {
    return switch (value) {
      null || '' || 'addressee' => MessageMentionRole.addressee,
      'cc' => MessageMentionRole.cc,
      _ => throw _invalidMention('unsupported mention role'),
    };
  }
}

class MessageMentionPayload {
  const MessageMentionPayload({
    required this.text,
    required this.mentions,
    this.annotations,
  });

  factory MessageMentionPayload.fromJson(Map<String, Object?> json) {
    final text = json['text'];
    final mentions = json['mentions'];
    final annotations = json['annotations'];
    if (text is! String) {
      throw _invalidMention('mention payload text must be a string');
    }
    if (mentions is! List) {
      throw _invalidMention('mention payload mentions must be an array');
    }
    if (annotations != null && annotations is! Map) {
      throw _invalidMention('mention payload annotations must be an object');
    }
    final payload = MessageMentionPayload(
      text: text,
      mentions: <MessageMention>[
        for (final mention in mentions)
          MessageMention.fromJson(_asMap(mention, 'mention')),
      ],
      annotations: annotations == null
          ? null
          : Map<String, Object?>.from(
              (annotations as Map).cast<String, Object?>(),
            ),
    );
    payload.validate();
    return payload;
  }

  final String text;
  final List<MessageMention> mentions;
  final Map<String, Object?>? annotations;

  Map<String, Object?> toJson() {
    validate();
    return <String, Object?>{
      'text': text,
      'mentions': <Object?>[for (final mention in mentions) mention.toJson()],
      if (annotations != null) 'annotations': annotations,
    };
  }

  String toPayloadJson() => jsonEncode(toJson());

  void validate() {
    if (text.isEmpty) {
      throw _invalidMention('text must not be empty');
    }
    if (mentions.isEmpty) {
      throw _invalidMention('mentions must not be empty');
    }
    final codePointLength = text.runes.length;
    final ids = <String>{};
    for (final mention in mentions) {
      mention.validate(codePointLength);
      if (!ids.add(mention.id)) {
        throw _invalidMention('duplicate mention id: ${mention.id}');
      }
    }
  }
}

class MessageMention {
  const MessageMention({
    required this.id,
    required this.range,
    required this.target,
    this.mentionRole = MessageMentionRole.addressee,
  });

  factory MessageMention.fromJson(Map<String, Object?> json) {
    for (final field in _forbiddenMentionFields) {
      if (json.containsKey(field)) {
        throw _invalidMention(
          'mention must not contain forbidden field `$field`',
        );
      }
    }
    final id = json['id'];
    if (id is! String) {
      throw _invalidMention('mention id must be a string');
    }
    final role = json['mention_role'];
    if (role != null && role is! String) {
      throw _invalidMention('mention_role must be a string');
    }
    return MessageMention(
      id: id,
      range: MessageMentionRange.fromJson(_asMap(json['range'], 'range')),
      target: MessageMentionTarget.fromJson(_asMap(json['target'], 'target')),
      mentionRole: MessageMentionRole.fromWireName(role as String?),
    );
  }

  final String id;
  final MessageMentionRange range;
  final MessageMentionTarget target;
  final MessageMentionRole mentionRole;

  Map<String, Object?> toJson() => <String, Object?>{
    'id': id,
    'range': range.toJson(),
    'target': target.toJson(),
    if (mentionRole != MessageMentionRole.addressee)
      'mention_role': mentionRole.wireName,
  };

  void validate(int textCodePoints) {
    if (id.trim().isEmpty) {
      throw _invalidMention('mention id must not be empty');
    }
    range.validate(textCodePoints);
    target.validate();
  }
}

class MessageMentionRange {
  const MessageMentionRange({
    required this.start,
    required this.end,
    this.unit = messageMentionRangeUnitUnicodeCodePoint,
  });

  factory MessageMentionRange.fromJson(Map<String, Object?> json) {
    final start = json['start'];
    final end = json['end'];
    final unit = json['unit'];
    if (start is! int || end is! int) {
      throw _invalidMention('mention range start/end must be integers');
    }
    if (unit is! String) {
      throw _invalidMention('mention range unit must be a string');
    }
    return MessageMentionRange(start: start, end: end, unit: unit);
  }

  final int start;
  final int end;
  final String unit;

  Map<String, Object?> toJson() => <String, Object?>{
    'start': start,
    'end': end,
    'unit': unit,
  };

  void validate(int textCodePoints) {
    if (start < 0 || end < 0) {
      throw _invalidMention('mention range start/end must be non-negative');
    }
    if (start >= end) {
      throw _invalidMention('mention range start must be less than end');
    }
    if (end > textCodePoints) {
      throw _invalidMention('mention range end exceeds text length');
    }
    if (unit != messageMentionRangeUnitUnicodeCodePoint) {
      throw _invalidMention('mention range unit must be unicode_code_point');
    }
  }
}

sealed class MessageMentionTarget {
  const MessageMentionTarget();

  const factory MessageMentionTarget.human({
    required String did,
    String? displayName,
  }) = HumanMessageMentionTarget;

  const factory MessageMentionTarget.agent({
    required String did,
    String? displayName,
  }) = AgentMessageMentionTarget;

  const factory MessageMentionTarget.groupSelector(
    MessageMentionSelector selector,
  ) = GroupSelectorMessageMentionTarget;

  factory MessageMentionTarget.fromJson(Map<String, Object?> json) {
    final kind = json['kind'];
    return switch (kind) {
      'human' => _didTargetFromJson(json, isAgent: false),
      'agent' => _didTargetFromJson(json, isAgent: true),
      'group_selector' => _selectorTargetFromJson(json),
      _ => throw _invalidMention('unsupported mention target kind'),
    };
  }

  Map<String, Object?> toJson();
  void validate();
}

class HumanMessageMentionTarget extends MessageMentionTarget {
  const HumanMessageMentionTarget({required this.did, this.displayName});

  final String did;
  final String? displayName;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'human',
    'did': did,
    if (displayName != null && displayName!.trim().isNotEmpty)
      'display_name': displayName,
  };

  @override
  void validate() {
    if (!_looksLikeDid(did)) {
      throw _invalidMention('human mention target did must be a DID');
    }
  }
}

class AgentMessageMentionTarget extends MessageMentionTarget {
  const AgentMessageMentionTarget({required this.did, this.displayName});

  final String did;
  final String? displayName;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'agent',
    'did': did,
    if (displayName != null && displayName!.trim().isNotEmpty)
      'display_name': displayName,
  };

  @override
  void validate() {
    if (!_looksLikeDid(did)) {
      throw _invalidMention('agent mention target did must be a DID');
    }
  }
}

class GroupSelectorMessageMentionTarget extends MessageMentionTarget {
  const GroupSelectorMessageMentionTarget(this.selector);

  final MessageMentionSelector selector;

  @override
  Map<String, Object?> toJson() => <String, Object?>{
    'kind': 'group_selector',
    'selector': selector.wireName,
  };

  @override
  void validate() {}
}

MessageMentionPayload decodeMessageMentionPayloadJson(String payloadJson) {
  return MessageMentionPayload.fromJson(
    decodeMessagePayloadObject(payloadJson),
  );
}

void validateMessageMentionPayloadJson(String payloadJson) {
  decodeMessageMentionPayloadJson(payloadJson);
}

MessageMentionTarget _didTargetFromJson(
  Map<String, Object?> json, {
  required bool isAgent,
}) {
  if (json.containsKey('selector')) {
    throw _invalidMention(
      'human/agent mention target must not contain selector',
    );
  }
  final did = json['did'];
  if (did is! String) {
    throw _invalidMention('human/agent mention target did must be a string');
  }
  final displayName = json['display_name'];
  if (displayName != null && displayName is! String) {
    throw _invalidMention('mention target display_name must be a string');
  }
  return isAgent
      ? MessageMentionTarget.agent(
          did: did,
          displayName: displayName as String?,
        )
      : MessageMentionTarget.human(
          did: did,
          displayName: displayName as String?,
        );
}

MessageMentionTarget _selectorTargetFromJson(Map<String, Object?> json) {
  if (json.containsKey('did')) {
    throw _invalidMention('group selector mention target must not contain did');
  }
  final selector = json['selector'];
  if (selector is! String) {
    throw _invalidMention(
      'group selector mention target selector must be a string',
    );
  }
  return MessageMentionTarget.groupSelector(
    MessageMentionSelector.fromWireName(selector),
  );
}

Map<String, Object?> _asMap(Object? value, String field) {
  if (value is! Map) {
    throw _invalidMention('$field must be an object');
  }
  return Map<String, Object?>.from(value.cast<String, Object?>());
}

bool _looksLikeDid(String value) {
  final trimmed = value.trim();
  return trimmed.startsWith('did:') && trimmed.length > 'did:'.length;
}

AwikiImCoreException _invalidMention(String message) {
  return AwikiImCoreException(code: 'invalid_mention', message: message);
}
