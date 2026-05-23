// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'message.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$DartMessageTarget {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartMessageTarget);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartMessageTarget()';
}


}

/// @nodoc
class $DartMessageTargetCopyWith<$Res>  {
$DartMessageTargetCopyWith(DartMessageTarget _, $Res Function(DartMessageTarget) __);
}


/// Adds pattern-matching-related methods to [DartMessageTarget].
extension DartMessageTargetPatterns on DartMessageTarget {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartMessageTarget_Direct value)?  direct,TResult Function( DartMessageTarget_Group value)?  group,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartMessageTarget_Direct() when direct != null:
return direct(_that);case DartMessageTarget_Group() when group != null:
return group(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartMessageTarget_Direct value)  direct,required TResult Function( DartMessageTarget_Group value)  group,}){
final _that = this;
switch (_that) {
case DartMessageTarget_Direct():
return direct(_that);case DartMessageTarget_Group():
return group(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartMessageTarget_Direct value)?  direct,TResult? Function( DartMessageTarget_Group value)?  group,}){
final _that = this;
switch (_that) {
case DartMessageTarget_Direct() when direct != null:
return direct(_that);case DartMessageTarget_Group() when group != null:
return group(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String peer)?  direct,TResult Function( String group)?  group,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartMessageTarget_Direct() when direct != null:
return direct(_that.peer);case DartMessageTarget_Group() when group != null:
return group(_that.group);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String peer)  direct,required TResult Function( String group)  group,}) {final _that = this;
switch (_that) {
case DartMessageTarget_Direct():
return direct(_that.peer);case DartMessageTarget_Group():
return group(_that.group);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String peer)?  direct,TResult? Function( String group)?  group,}) {final _that = this;
switch (_that) {
case DartMessageTarget_Direct() when direct != null:
return direct(_that.peer);case DartMessageTarget_Group() when group != null:
return group(_that.group);case _:
  return null;

}
}

}

/// @nodoc


class DartMessageTarget_Direct extends DartMessageTarget {
  const DartMessageTarget_Direct({required this.peer}): super._();


 final  String peer;

/// Create a copy of DartMessageTarget
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartMessageTarget_DirectCopyWith<DartMessageTarget_Direct> get copyWith => _$DartMessageTarget_DirectCopyWithImpl<DartMessageTarget_Direct>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartMessageTarget_Direct&&(identical(other.peer, peer) || other.peer == peer));
}


@override
int get hashCode => Object.hash(runtimeType,peer);

@override
String toString() {
  return 'DartMessageTarget.direct(peer: $peer)';
}


}

/// @nodoc
abstract mixin class $DartMessageTarget_DirectCopyWith<$Res> implements $DartMessageTargetCopyWith<$Res> {
  factory $DartMessageTarget_DirectCopyWith(DartMessageTarget_Direct value, $Res Function(DartMessageTarget_Direct) _then) = _$DartMessageTarget_DirectCopyWithImpl;
@useResult
$Res call({
 String peer
});




}
/// @nodoc
class _$DartMessageTarget_DirectCopyWithImpl<$Res>
    implements $DartMessageTarget_DirectCopyWith<$Res> {
  _$DartMessageTarget_DirectCopyWithImpl(this._self, this._then);

  final DartMessageTarget_Direct _self;
  final $Res Function(DartMessageTarget_Direct) _then;

/// Create a copy of DartMessageTarget
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? peer = null,}) {
  return _then(DartMessageTarget_Direct(
peer: null == peer ? _self.peer : peer // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartMessageTarget_Group extends DartMessageTarget {
  const DartMessageTarget_Group({required this.group}): super._();


 final  String group;

/// Create a copy of DartMessageTarget
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartMessageTarget_GroupCopyWith<DartMessageTarget_Group> get copyWith => _$DartMessageTarget_GroupCopyWithImpl<DartMessageTarget_Group>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartMessageTarget_Group&&(identical(other.group, group) || other.group == group));
}


@override
int get hashCode => Object.hash(runtimeType,group);

@override
String toString() {
  return 'DartMessageTarget.group(group: $group)';
}


}

/// @nodoc
abstract mixin class $DartMessageTarget_GroupCopyWith<$Res> implements $DartMessageTargetCopyWith<$Res> {
  factory $DartMessageTarget_GroupCopyWith(DartMessageTarget_Group value, $Res Function(DartMessageTarget_Group) _then) = _$DartMessageTarget_GroupCopyWithImpl;
@useResult
$Res call({
 String group
});




}
/// @nodoc
class _$DartMessageTarget_GroupCopyWithImpl<$Res>
    implements $DartMessageTarget_GroupCopyWith<$Res> {
  _$DartMessageTarget_GroupCopyWithImpl(this._self, this._then);

  final DartMessageTarget_Group _self;
  final $Res Function(DartMessageTarget_Group) _then;

/// Create a copy of DartMessageTarget
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? group = null,}) {
  return _then(DartMessageTarget_Group(
group: null == group ? _self.group : group // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$DartThreadRef {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadRef);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartThreadRef()';
}


}

/// @nodoc
class $DartThreadRefCopyWith<$Res>  {
$DartThreadRefCopyWith(DartThreadRef _, $Res Function(DartThreadRef) __);
}


/// Adds pattern-matching-related methods to [DartThreadRef].
extension DartThreadRefPatterns on DartThreadRef {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartThreadRef_Direct value)?  direct,TResult Function( DartThreadRef_Group value)?  group,TResult Function( DartThreadRef_Thread value)?  thread,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartThreadRef_Direct() when direct != null:
return direct(_that);case DartThreadRef_Group() when group != null:
return group(_that);case DartThreadRef_Thread() when thread != null:
return thread(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartThreadRef_Direct value)  direct,required TResult Function( DartThreadRef_Group value)  group,required TResult Function( DartThreadRef_Thread value)  thread,}){
final _that = this;
switch (_that) {
case DartThreadRef_Direct():
return direct(_that);case DartThreadRef_Group():
return group(_that);case DartThreadRef_Thread():
return thread(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartThreadRef_Direct value)?  direct,TResult? Function( DartThreadRef_Group value)?  group,TResult? Function( DartThreadRef_Thread value)?  thread,}){
final _that = this;
switch (_that) {
case DartThreadRef_Direct() when direct != null:
return direct(_that);case DartThreadRef_Group() when group != null:
return group(_that);case DartThreadRef_Thread() when thread != null:
return thread(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String peer)?  direct,TResult Function( String group)?  group,TResult Function( String threadId)?  thread,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartThreadRef_Direct() when direct != null:
return direct(_that.peer);case DartThreadRef_Group() when group != null:
return group(_that.group);case DartThreadRef_Thread() when thread != null:
return thread(_that.threadId);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String peer)  direct,required TResult Function( String group)  group,required TResult Function( String threadId)  thread,}) {final _that = this;
switch (_that) {
case DartThreadRef_Direct():
return direct(_that.peer);case DartThreadRef_Group():
return group(_that.group);case DartThreadRef_Thread():
return thread(_that.threadId);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String peer)?  direct,TResult? Function( String group)?  group,TResult? Function( String threadId)?  thread,}) {final _that = this;
switch (_that) {
case DartThreadRef_Direct() when direct != null:
return direct(_that.peer);case DartThreadRef_Group() when group != null:
return group(_that.group);case DartThreadRef_Thread() when thread != null:
return thread(_that.threadId);case _:
  return null;

}
}

}

/// @nodoc


class DartThreadRef_Direct extends DartThreadRef {
  const DartThreadRef_Direct({required this.peer}): super._();


 final  String peer;

/// Create a copy of DartThreadRef
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadRef_DirectCopyWith<DartThreadRef_Direct> get copyWith => _$DartThreadRef_DirectCopyWithImpl<DartThreadRef_Direct>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadRef_Direct&&(identical(other.peer, peer) || other.peer == peer));
}


@override
int get hashCode => Object.hash(runtimeType,peer);

@override
String toString() {
  return 'DartThreadRef.direct(peer: $peer)';
}


}

/// @nodoc
abstract mixin class $DartThreadRef_DirectCopyWith<$Res> implements $DartThreadRefCopyWith<$Res> {
  factory $DartThreadRef_DirectCopyWith(DartThreadRef_Direct value, $Res Function(DartThreadRef_Direct) _then) = _$DartThreadRef_DirectCopyWithImpl;
@useResult
$Res call({
 String peer
});




}
/// @nodoc
class _$DartThreadRef_DirectCopyWithImpl<$Res>
    implements $DartThreadRef_DirectCopyWith<$Res> {
  _$DartThreadRef_DirectCopyWithImpl(this._self, this._then);

  final DartThreadRef_Direct _self;
  final $Res Function(DartThreadRef_Direct) _then;

/// Create a copy of DartThreadRef
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? peer = null,}) {
  return _then(DartThreadRef_Direct(
peer: null == peer ? _self.peer : peer // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartThreadRef_Group extends DartThreadRef {
  const DartThreadRef_Group({required this.group}): super._();


 final  String group;

/// Create a copy of DartThreadRef
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadRef_GroupCopyWith<DartThreadRef_Group> get copyWith => _$DartThreadRef_GroupCopyWithImpl<DartThreadRef_Group>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadRef_Group&&(identical(other.group, group) || other.group == group));
}


@override
int get hashCode => Object.hash(runtimeType,group);

@override
String toString() {
  return 'DartThreadRef.group(group: $group)';
}


}

/// @nodoc
abstract mixin class $DartThreadRef_GroupCopyWith<$Res> implements $DartThreadRefCopyWith<$Res> {
  factory $DartThreadRef_GroupCopyWith(DartThreadRef_Group value, $Res Function(DartThreadRef_Group) _then) = _$DartThreadRef_GroupCopyWithImpl;
@useResult
$Res call({
 String group
});




}
/// @nodoc
class _$DartThreadRef_GroupCopyWithImpl<$Res>
    implements $DartThreadRef_GroupCopyWith<$Res> {
  _$DartThreadRef_GroupCopyWithImpl(this._self, this._then);

  final DartThreadRef_Group _self;
  final $Res Function(DartThreadRef_Group) _then;

/// Create a copy of DartThreadRef
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? group = null,}) {
  return _then(DartThreadRef_Group(
group: null == group ? _self.group : group // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartThreadRef_Thread extends DartThreadRef {
  const DartThreadRef_Thread({required this.threadId}): super._();


 final  String threadId;

/// Create a copy of DartThreadRef
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadRef_ThreadCopyWith<DartThreadRef_Thread> get copyWith => _$DartThreadRef_ThreadCopyWithImpl<DartThreadRef_Thread>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadRef_Thread&&(identical(other.threadId, threadId) || other.threadId == threadId));
}


@override
int get hashCode => Object.hash(runtimeType,threadId);

@override
String toString() {
  return 'DartThreadRef.thread(threadId: $threadId)';
}


}

/// @nodoc
abstract mixin class $DartThreadRef_ThreadCopyWith<$Res> implements $DartThreadRefCopyWith<$Res> {
  factory $DartThreadRef_ThreadCopyWith(DartThreadRef_Thread value, $Res Function(DartThreadRef_Thread) _then) = _$DartThreadRef_ThreadCopyWithImpl;
@useResult
$Res call({
 String threadId
});




}
/// @nodoc
class _$DartThreadRef_ThreadCopyWithImpl<$Res>
    implements $DartThreadRef_ThreadCopyWith<$Res> {
  _$DartThreadRef_ThreadCopyWithImpl(this._self, this._then);

  final DartThreadRef_Thread _self;
  final $Res Function(DartThreadRef_Thread) _then;

/// Create a copy of DartThreadRef
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? threadId = null,}) {
  return _then(DartThreadRef_Thread(
threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on
