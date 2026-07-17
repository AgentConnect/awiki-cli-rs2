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
mixin _$DartConversationStorePatch {

 String get ownerIdentityId; String get ownerDid; BigInt get version; int get unreadTotal;
/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartConversationStorePatchCopyWith<DartConversationStorePatch> get copyWith => _$DartConversationStorePatchCopyWithImpl<DartConversationStorePatch>(this as DartConversationStorePatch, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartConversationStorePatch&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.unreadTotal, unreadTotal) || other.unreadTotal == unreadTotal));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,unreadTotal);

@override
String toString() {
  return 'DartConversationStorePatch(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, unreadTotal: $unreadTotal)';
}


}

/// @nodoc
abstract mixin class $DartConversationStorePatchCopyWith<$Res>  {
  factory $DartConversationStorePatchCopyWith(DartConversationStorePatch value, $Res Function(DartConversationStorePatch) _then) = _$DartConversationStorePatchCopyWithImpl;
@useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, int unreadTotal
});




}
/// @nodoc
class _$DartConversationStorePatchCopyWithImpl<$Res>
    implements $DartConversationStorePatchCopyWith<$Res> {
  _$DartConversationStorePatchCopyWithImpl(this._self, this._then);

  final DartConversationStorePatch _self;
  final $Res Function(DartConversationStorePatch) _then;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? unreadTotal = null,}) {
  return _then(_self.copyWith(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,unreadTotal: null == unreadTotal ? _self.unreadTotal : unreadTotal // ignore: cast_nullable_to_non_nullable
as int,
  ));
}

}


/// Adds pattern-matching-related methods to [DartConversationStorePatch].
extension DartConversationStorePatchPatterns on DartConversationStorePatch {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartConversationStorePatch_Reset value)?  reset,TResult Function( DartConversationStorePatch_Upsert value)?  upsert,TResult Function( DartConversationStorePatch_Remove value)?  remove,TResult Function( DartConversationStorePatch_Reorder value)?  reorder,TResult Function( DartConversationStorePatch_RepairRequired value)?  repairRequired,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartConversationStorePatch_Reset() when reset != null:
return reset(_that);case DartConversationStorePatch_Upsert() when upsert != null:
return upsert(_that);case DartConversationStorePatch_Remove() when remove != null:
return remove(_that);case DartConversationStorePatch_Reorder() when reorder != null:
return reorder(_that);case DartConversationStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartConversationStorePatch_Reset value)  reset,required TResult Function( DartConversationStorePatch_Upsert value)  upsert,required TResult Function( DartConversationStorePatch_Remove value)  remove,required TResult Function( DartConversationStorePatch_Reorder value)  reorder,required TResult Function( DartConversationStorePatch_RepairRequired value)  repairRequired,}){
final _that = this;
switch (_that) {
case DartConversationStorePatch_Reset():
return reset(_that);case DartConversationStorePatch_Upsert():
return upsert(_that);case DartConversationStorePatch_Remove():
return remove(_that);case DartConversationStorePatch_Reorder():
return reorder(_that);case DartConversationStorePatch_RepairRequired():
return repairRequired(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartConversationStorePatch_Reset value)?  reset,TResult? Function( DartConversationStorePatch_Upsert value)?  upsert,TResult? Function( DartConversationStorePatch_Remove value)?  remove,TResult? Function( DartConversationStorePatch_Reorder value)?  reorder,TResult? Function( DartConversationStorePatch_RepairRequired value)?  repairRequired,}){
final _that = this;
switch (_that) {
case DartConversationStorePatch_Reset() when reset != null:
return reset(_that);case DartConversationStorePatch_Upsert() when upsert != null:
return upsert(_that);case DartConversationStorePatch_Remove() when remove != null:
return remove(_that);case DartConversationStorePatch_Reorder() when reorder != null:
return reorder(_that);case DartConversationStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  List<DartConversationSnapshotItem> items)?  reset,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  DartConversationSnapshotItem item,  int index)?  upsert,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String conversationId)?  remove,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String conversationId,  int index)?  reorder,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String reason)?  repairRequired,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartConversationStorePatch_Reset() when reset != null:
return reset(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.items);case DartConversationStorePatch_Upsert() when upsert != null:
return upsert(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.item,_that.index);case DartConversationStorePatch_Remove() when remove != null:
return remove(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.conversationId);case DartConversationStorePatch_Reorder() when reorder != null:
return reorder(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.conversationId,_that.index);case DartConversationStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  List<DartConversationSnapshotItem> items)  reset,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  DartConversationSnapshotItem item,  int index)  upsert,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String conversationId)  remove,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String conversationId,  int index)  reorder,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String reason)  repairRequired,}) {final _that = this;
switch (_that) {
case DartConversationStorePatch_Reset():
return reset(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.items);case DartConversationStorePatch_Upsert():
return upsert(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.item,_that.index);case DartConversationStorePatch_Remove():
return remove(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.conversationId);case DartConversationStorePatch_Reorder():
return reorder(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.conversationId,_that.index);case DartConversationStorePatch_RepairRequired():
return repairRequired(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  List<DartConversationSnapshotItem> items)?  reset,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  DartConversationSnapshotItem item,  int index)?  upsert,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String conversationId)?  remove,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String conversationId,  int index)?  reorder,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  int unreadTotal,  String reason)?  repairRequired,}) {final _that = this;
switch (_that) {
case DartConversationStorePatch_Reset() when reset != null:
return reset(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.items);case DartConversationStorePatch_Upsert() when upsert != null:
return upsert(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.item,_that.index);case DartConversationStorePatch_Remove() when remove != null:
return remove(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.conversationId);case DartConversationStorePatch_Reorder() when reorder != null:
return reorder(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.conversationId,_that.index);case DartConversationStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.unreadTotal,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class DartConversationStorePatch_Reset extends DartConversationStorePatch {
  const DartConversationStorePatch_Reset({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.unreadTotal, required final  List<DartConversationSnapshotItem> items}): _items = items,super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  int unreadTotal;
 final  List<DartConversationSnapshotItem> _items;
 List<DartConversationSnapshotItem> get items {
  if (_items is EqualUnmodifiableListView) return _items;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_items);
}


/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartConversationStorePatch_ResetCopyWith<DartConversationStorePatch_Reset> get copyWith => _$DartConversationStorePatch_ResetCopyWithImpl<DartConversationStorePatch_Reset>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartConversationStorePatch_Reset&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.unreadTotal, unreadTotal) || other.unreadTotal == unreadTotal)&&const DeepCollectionEquality().equals(other._items, _items));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,unreadTotal,const DeepCollectionEquality().hash(_items));

@override
String toString() {
  return 'DartConversationStorePatch.reset(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, unreadTotal: $unreadTotal, items: $items)';
}


}

/// @nodoc
abstract mixin class $DartConversationStorePatch_ResetCopyWith<$Res> implements $DartConversationStorePatchCopyWith<$Res> {
  factory $DartConversationStorePatch_ResetCopyWith(DartConversationStorePatch_Reset value, $Res Function(DartConversationStorePatch_Reset) _then) = _$DartConversationStorePatch_ResetCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, int unreadTotal, List<DartConversationSnapshotItem> items
});




}
/// @nodoc
class _$DartConversationStorePatch_ResetCopyWithImpl<$Res>
    implements $DartConversationStorePatch_ResetCopyWith<$Res> {
  _$DartConversationStorePatch_ResetCopyWithImpl(this._self, this._then);

  final DartConversationStorePatch_Reset _self;
  final $Res Function(DartConversationStorePatch_Reset) _then;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? unreadTotal = null,Object? items = null,}) {
  return _then(DartConversationStorePatch_Reset(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,unreadTotal: null == unreadTotal ? _self.unreadTotal : unreadTotal // ignore: cast_nullable_to_non_nullable
as int,items: null == items ? _self._items : items // ignore: cast_nullable_to_non_nullable
as List<DartConversationSnapshotItem>,
  ));
}


}

/// @nodoc


class DartConversationStorePatch_Upsert extends DartConversationStorePatch {
  const DartConversationStorePatch_Upsert({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.unreadTotal, required this.item, required this.index}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  int unreadTotal;
 final  DartConversationSnapshotItem item;
 final  int index;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartConversationStorePatch_UpsertCopyWith<DartConversationStorePatch_Upsert> get copyWith => _$DartConversationStorePatch_UpsertCopyWithImpl<DartConversationStorePatch_Upsert>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartConversationStorePatch_Upsert&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.unreadTotal, unreadTotal) || other.unreadTotal == unreadTotal)&&(identical(other.item, item) || other.item == item)&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,unreadTotal,item,index);

@override
String toString() {
  return 'DartConversationStorePatch.upsert(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, unreadTotal: $unreadTotal, item: $item, index: $index)';
}


}

/// @nodoc
abstract mixin class $DartConversationStorePatch_UpsertCopyWith<$Res> implements $DartConversationStorePatchCopyWith<$Res> {
  factory $DartConversationStorePatch_UpsertCopyWith(DartConversationStorePatch_Upsert value, $Res Function(DartConversationStorePatch_Upsert) _then) = _$DartConversationStorePatch_UpsertCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, int unreadTotal, DartConversationSnapshotItem item, int index
});




}
/// @nodoc
class _$DartConversationStorePatch_UpsertCopyWithImpl<$Res>
    implements $DartConversationStorePatch_UpsertCopyWith<$Res> {
  _$DartConversationStorePatch_UpsertCopyWithImpl(this._self, this._then);

  final DartConversationStorePatch_Upsert _self;
  final $Res Function(DartConversationStorePatch_Upsert) _then;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? unreadTotal = null,Object? item = null,Object? index = null,}) {
  return _then(DartConversationStorePatch_Upsert(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,unreadTotal: null == unreadTotal ? _self.unreadTotal : unreadTotal // ignore: cast_nullable_to_non_nullable
as int,item: null == item ? _self.item : item // ignore: cast_nullable_to_non_nullable
as DartConversationSnapshotItem,index: null == index ? _self.index : index // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class DartConversationStorePatch_Remove extends DartConversationStorePatch {
  const DartConversationStorePatch_Remove({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.unreadTotal, required this.conversationId}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  int unreadTotal;
 final  String conversationId;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartConversationStorePatch_RemoveCopyWith<DartConversationStorePatch_Remove> get copyWith => _$DartConversationStorePatch_RemoveCopyWithImpl<DartConversationStorePatch_Remove>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartConversationStorePatch_Remove&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.unreadTotal, unreadTotal) || other.unreadTotal == unreadTotal)&&(identical(other.conversationId, conversationId) || other.conversationId == conversationId));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,unreadTotal,conversationId);

@override
String toString() {
  return 'DartConversationStorePatch.remove(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, unreadTotal: $unreadTotal, conversationId: $conversationId)';
}


}

/// @nodoc
abstract mixin class $DartConversationStorePatch_RemoveCopyWith<$Res> implements $DartConversationStorePatchCopyWith<$Res> {
  factory $DartConversationStorePatch_RemoveCopyWith(DartConversationStorePatch_Remove value, $Res Function(DartConversationStorePatch_Remove) _then) = _$DartConversationStorePatch_RemoveCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, int unreadTotal, String conversationId
});




}
/// @nodoc
class _$DartConversationStorePatch_RemoveCopyWithImpl<$Res>
    implements $DartConversationStorePatch_RemoveCopyWith<$Res> {
  _$DartConversationStorePatch_RemoveCopyWithImpl(this._self, this._then);

  final DartConversationStorePatch_Remove _self;
  final $Res Function(DartConversationStorePatch_Remove) _then;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? unreadTotal = null,Object? conversationId = null,}) {
  return _then(DartConversationStorePatch_Remove(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,unreadTotal: null == unreadTotal ? _self.unreadTotal : unreadTotal // ignore: cast_nullable_to_non_nullable
as int,conversationId: null == conversationId ? _self.conversationId : conversationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartConversationStorePatch_Reorder extends DartConversationStorePatch {
  const DartConversationStorePatch_Reorder({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.unreadTotal, required this.conversationId, required this.index}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  int unreadTotal;
 final  String conversationId;
 final  int index;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartConversationStorePatch_ReorderCopyWith<DartConversationStorePatch_Reorder> get copyWith => _$DartConversationStorePatch_ReorderCopyWithImpl<DartConversationStorePatch_Reorder>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartConversationStorePatch_Reorder&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.unreadTotal, unreadTotal) || other.unreadTotal == unreadTotal)&&(identical(other.conversationId, conversationId) || other.conversationId == conversationId)&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,unreadTotal,conversationId,index);

@override
String toString() {
  return 'DartConversationStorePatch.reorder(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, unreadTotal: $unreadTotal, conversationId: $conversationId, index: $index)';
}


}

/// @nodoc
abstract mixin class $DartConversationStorePatch_ReorderCopyWith<$Res> implements $DartConversationStorePatchCopyWith<$Res> {
  factory $DartConversationStorePatch_ReorderCopyWith(DartConversationStorePatch_Reorder value, $Res Function(DartConversationStorePatch_Reorder) _then) = _$DartConversationStorePatch_ReorderCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, int unreadTotal, String conversationId, int index
});




}
/// @nodoc
class _$DartConversationStorePatch_ReorderCopyWithImpl<$Res>
    implements $DartConversationStorePatch_ReorderCopyWith<$Res> {
  _$DartConversationStorePatch_ReorderCopyWithImpl(this._self, this._then);

  final DartConversationStorePatch_Reorder _self;
  final $Res Function(DartConversationStorePatch_Reorder) _then;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? unreadTotal = null,Object? conversationId = null,Object? index = null,}) {
  return _then(DartConversationStorePatch_Reorder(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,unreadTotal: null == unreadTotal ? _self.unreadTotal : unreadTotal // ignore: cast_nullable_to_non_nullable
as int,conversationId: null == conversationId ? _self.conversationId : conversationId // ignore: cast_nullable_to_non_nullable
as String,index: null == index ? _self.index : index // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class DartConversationStorePatch_RepairRequired extends DartConversationStorePatch {
  const DartConversationStorePatch_RepairRequired({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.unreadTotal, required this.reason}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  int unreadTotal;
 final  String reason;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartConversationStorePatch_RepairRequiredCopyWith<DartConversationStorePatch_RepairRequired> get copyWith => _$DartConversationStorePatch_RepairRequiredCopyWithImpl<DartConversationStorePatch_RepairRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartConversationStorePatch_RepairRequired&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.unreadTotal, unreadTotal) || other.unreadTotal == unreadTotal)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,unreadTotal,reason);

@override
String toString() {
  return 'DartConversationStorePatch.repairRequired(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, unreadTotal: $unreadTotal, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $DartConversationStorePatch_RepairRequiredCopyWith<$Res> implements $DartConversationStorePatchCopyWith<$Res> {
  factory $DartConversationStorePatch_RepairRequiredCopyWith(DartConversationStorePatch_RepairRequired value, $Res Function(DartConversationStorePatch_RepairRequired) _then) = _$DartConversationStorePatch_RepairRequiredCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, int unreadTotal, String reason
});




}
/// @nodoc
class _$DartConversationStorePatch_RepairRequiredCopyWithImpl<$Res>
    implements $DartConversationStorePatch_RepairRequiredCopyWith<$Res> {
  _$DartConversationStorePatch_RepairRequiredCopyWithImpl(this._self, this._then);

  final DartConversationStorePatch_RepairRequired _self;
  final $Res Function(DartConversationStorePatch_RepairRequired) _then;

/// Create a copy of DartConversationStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? unreadTotal = null,Object? reason = null,}) {
  return _then(DartConversationStorePatch_RepairRequired(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,unreadTotal: null == unreadTotal ? _self.unreadTotal : unreadTotal // ignore: cast_nullable_to_non_nullable
as int,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$DartInboxAuth {

 DartScopedInboxToken get token;
/// Create a copy of DartInboxAuth
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartInboxAuthCopyWith<DartInboxAuth> get copyWith => _$DartInboxAuthCopyWithImpl<DartInboxAuth>(this as DartInboxAuth, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartInboxAuth&&(identical(other.token, token) || other.token == token));
}


@override
int get hashCode => Object.hash(runtimeType,token);

@override
String toString() {
  return 'DartInboxAuth(token: $token)';
}


}

/// @nodoc
abstract mixin class $DartInboxAuthCopyWith<$Res>  {
  factory $DartInboxAuthCopyWith(DartInboxAuth value, $Res Function(DartInboxAuth) _then) = _$DartInboxAuthCopyWithImpl;
@useResult
$Res call({
 DartScopedInboxToken token
});




}
/// @nodoc
class _$DartInboxAuthCopyWithImpl<$Res>
    implements $DartInboxAuthCopyWith<$Res> {
  _$DartInboxAuthCopyWithImpl(this._self, this._then);

  final DartInboxAuth _self;
  final $Res Function(DartInboxAuth) _then;

/// Create a copy of DartInboxAuth
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? token = null,}) {
  return _then(_self.copyWith(
token: null == token ? _self.token : token // ignore: cast_nullable_to_non_nullable
as DartScopedInboxToken,
  ));
}

}


/// Adds pattern-matching-related methods to [DartInboxAuth].
extension DartInboxAuthPatterns on DartInboxAuth {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartInboxAuth_ScopedInboxToken value)?  scopedInboxToken,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartInboxAuth_ScopedInboxToken() when scopedInboxToken != null:
return scopedInboxToken(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartInboxAuth_ScopedInboxToken value)  scopedInboxToken,}){
final _that = this;
switch (_that) {
case DartInboxAuth_ScopedInboxToken():
return scopedInboxToken(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartInboxAuth_ScopedInboxToken value)?  scopedInboxToken,}){
final _that = this;
switch (_that) {
case DartInboxAuth_ScopedInboxToken() when scopedInboxToken != null:
return scopedInboxToken(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( DartScopedInboxToken token)?  scopedInboxToken,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartInboxAuth_ScopedInboxToken() when scopedInboxToken != null:
return scopedInboxToken(_that.token);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( DartScopedInboxToken token)  scopedInboxToken,}) {final _that = this;
switch (_that) {
case DartInboxAuth_ScopedInboxToken():
return scopedInboxToken(_that.token);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( DartScopedInboxToken token)?  scopedInboxToken,}) {final _that = this;
switch (_that) {
case DartInboxAuth_ScopedInboxToken() when scopedInboxToken != null:
return scopedInboxToken(_that.token);case _:
  return null;

}
}

}

/// @nodoc


class DartInboxAuth_ScopedInboxToken extends DartInboxAuth {
  const DartInboxAuth_ScopedInboxToken({required this.token}): super._();


@override final  DartScopedInboxToken token;

/// Create a copy of DartInboxAuth
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartInboxAuth_ScopedInboxTokenCopyWith<DartInboxAuth_ScopedInboxToken> get copyWith => _$DartInboxAuth_ScopedInboxTokenCopyWithImpl<DartInboxAuth_ScopedInboxToken>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartInboxAuth_ScopedInboxToken&&(identical(other.token, token) || other.token == token));
}


@override
int get hashCode => Object.hash(runtimeType,token);

@override
String toString() {
  return 'DartInboxAuth.scopedInboxToken(token: $token)';
}


}

/// @nodoc
abstract mixin class $DartInboxAuth_ScopedInboxTokenCopyWith<$Res> implements $DartInboxAuthCopyWith<$Res> {
  factory $DartInboxAuth_ScopedInboxTokenCopyWith(DartInboxAuth_ScopedInboxToken value, $Res Function(DartInboxAuth_ScopedInboxToken) _then) = _$DartInboxAuth_ScopedInboxTokenCopyWithImpl;
@override @useResult
$Res call({
 DartScopedInboxToken token
});




}
/// @nodoc
class _$DartInboxAuth_ScopedInboxTokenCopyWithImpl<$Res>
    implements $DartInboxAuth_ScopedInboxTokenCopyWith<$Res> {
  _$DartInboxAuth_ScopedInboxTokenCopyWithImpl(this._self, this._then);

  final DartInboxAuth_ScopedInboxToken _self;
  final $Res Function(DartInboxAuth_ScopedInboxToken) _then;

/// Create a copy of DartInboxAuth
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? token = null,}) {
  return _then(DartInboxAuth_ScopedInboxToken(
token: null == token ? _self.token : token // ignore: cast_nullable_to_non_nullable
as DartScopedInboxToken,
  ));
}


}

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
mixin _$DartThreadMessageStorePatch {

 String get ownerIdentityId; String get ownerDid; BigInt get version; String get threadKind; String get threadId; DartConversationIdentity? get conversationIdentity;
/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadMessageStorePatchCopyWith<DartThreadMessageStorePatch> get copyWith => _$DartThreadMessageStorePatchCopyWithImpl<DartThreadMessageStorePatch>(this as DartThreadMessageStorePatch, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadMessageStorePatch&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.threadKind, threadKind) || other.threadKind == threadKind)&&(identical(other.threadId, threadId) || other.threadId == threadId)&&(identical(other.conversationIdentity, conversationIdentity) || other.conversationIdentity == conversationIdentity));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,threadKind,threadId,conversationIdentity);

@override
String toString() {
  return 'DartThreadMessageStorePatch(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, threadKind: $threadKind, threadId: $threadId, conversationIdentity: $conversationIdentity)';
}


}

/// @nodoc
abstract mixin class $DartThreadMessageStorePatchCopyWith<$Res>  {
  factory $DartThreadMessageStorePatchCopyWith(DartThreadMessageStorePatch value, $Res Function(DartThreadMessageStorePatch) _then) = _$DartThreadMessageStorePatchCopyWithImpl;
@useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, String threadKind, String threadId, DartConversationIdentity? conversationIdentity
});




}
/// @nodoc
class _$DartThreadMessageStorePatchCopyWithImpl<$Res>
    implements $DartThreadMessageStorePatchCopyWith<$Res> {
  _$DartThreadMessageStorePatchCopyWithImpl(this._self, this._then);

  final DartThreadMessageStorePatch _self;
  final $Res Function(DartThreadMessageStorePatch) _then;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? threadKind = null,Object? threadId = null,Object? conversationIdentity = freezed,}) {
  return _then(_self.copyWith(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,threadKind: null == threadKind ? _self.threadKind : threadKind // ignore: cast_nullable_to_non_nullable
as String,threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,conversationIdentity: freezed == conversationIdentity ? _self.conversationIdentity : conversationIdentity // ignore: cast_nullable_to_non_nullable
as DartConversationIdentity?,
  ));
}

}


/// Adds pattern-matching-related methods to [DartThreadMessageStorePatch].
extension DartThreadMessageStorePatchPatterns on DartThreadMessageStorePatch {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartThreadMessageStorePatch_Reset value)?  reset,TResult Function( DartThreadMessageStorePatch_Upsert value)?  upsert,TResult Function( DartThreadMessageStorePatch_Remove value)?  remove,TResult Function( DartThreadMessageStorePatch_RepairRequired value)?  repairRequired,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartThreadMessageStorePatch_Reset() when reset != null:
return reset(_that);case DartThreadMessageStorePatch_Upsert() when upsert != null:
return upsert(_that);case DartThreadMessageStorePatch_Remove() when remove != null:
return remove(_that);case DartThreadMessageStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartThreadMessageStorePatch_Reset value)  reset,required TResult Function( DartThreadMessageStorePatch_Upsert value)  upsert,required TResult Function( DartThreadMessageStorePatch_Remove value)  remove,required TResult Function( DartThreadMessageStorePatch_RepairRequired value)  repairRequired,}){
final _that = this;
switch (_that) {
case DartThreadMessageStorePatch_Reset():
return reset(_that);case DartThreadMessageStorePatch_Upsert():
return upsert(_that);case DartThreadMessageStorePatch_Remove():
return remove(_that);case DartThreadMessageStorePatch_RepairRequired():
return repairRequired(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartThreadMessageStorePatch_Reset value)?  reset,TResult? Function( DartThreadMessageStorePatch_Upsert value)?  upsert,TResult? Function( DartThreadMessageStorePatch_Remove value)?  remove,TResult? Function( DartThreadMessageStorePatch_RepairRequired value)?  repairRequired,}){
final _that = this;
switch (_that) {
case DartThreadMessageStorePatch_Reset() when reset != null:
return reset(_that);case DartThreadMessageStorePatch_Upsert() when upsert != null:
return upsert(_that);case DartThreadMessageStorePatch_Remove() when remove != null:
return remove(_that);case DartThreadMessageStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  List<DartMessage> items)?  reset,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  DartMessage message,  int index)?  upsert,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  String messageId)?  remove,TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  String reason)?  repairRequired,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartThreadMessageStorePatch_Reset() when reset != null:
return reset(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.items);case DartThreadMessageStorePatch_Upsert() when upsert != null:
return upsert(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.message,_that.index);case DartThreadMessageStorePatch_Remove() when remove != null:
return remove(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.messageId);case DartThreadMessageStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  List<DartMessage> items)  reset,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  DartMessage message,  int index)  upsert,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  String messageId)  remove,required TResult Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  String reason)  repairRequired,}) {final _that = this;
switch (_that) {
case DartThreadMessageStorePatch_Reset():
return reset(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.items);case DartThreadMessageStorePatch_Upsert():
return upsert(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.message,_that.index);case DartThreadMessageStorePatch_Remove():
return remove(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.messageId);case DartThreadMessageStorePatch_RepairRequired():
return repairRequired(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  List<DartMessage> items)?  reset,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  DartMessage message,  int index)?  upsert,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  String messageId)?  remove,TResult? Function( String ownerIdentityId,  String ownerDid,  BigInt version,  String threadKind,  String threadId,  DartConversationIdentity? conversationIdentity,  String reason)?  repairRequired,}) {final _that = this;
switch (_that) {
case DartThreadMessageStorePatch_Reset() when reset != null:
return reset(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.items);case DartThreadMessageStorePatch_Upsert() when upsert != null:
return upsert(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.message,_that.index);case DartThreadMessageStorePatch_Remove() when remove != null:
return remove(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.messageId);case DartThreadMessageStorePatch_RepairRequired() when repairRequired != null:
return repairRequired(_that.ownerIdentityId,_that.ownerDid,_that.version,_that.threadKind,_that.threadId,_that.conversationIdentity,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class DartThreadMessageStorePatch_Reset extends DartThreadMessageStorePatch {
  const DartThreadMessageStorePatch_Reset({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.threadKind, required this.threadId, this.conversationIdentity, required final  List<DartMessage> items}): _items = items,super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  String threadKind;
@override final  String threadId;
@override final  DartConversationIdentity? conversationIdentity;
 final  List<DartMessage> _items;
 List<DartMessage> get items {
  if (_items is EqualUnmodifiableListView) return _items;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_items);
}


/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadMessageStorePatch_ResetCopyWith<DartThreadMessageStorePatch_Reset> get copyWith => _$DartThreadMessageStorePatch_ResetCopyWithImpl<DartThreadMessageStorePatch_Reset>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadMessageStorePatch_Reset&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.threadKind, threadKind) || other.threadKind == threadKind)&&(identical(other.threadId, threadId) || other.threadId == threadId)&&(identical(other.conversationIdentity, conversationIdentity) || other.conversationIdentity == conversationIdentity)&&const DeepCollectionEquality().equals(other._items, _items));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,threadKind,threadId,conversationIdentity,const DeepCollectionEquality().hash(_items));

@override
String toString() {
  return 'DartThreadMessageStorePatch.reset(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, threadKind: $threadKind, threadId: $threadId, conversationIdentity: $conversationIdentity, items: $items)';
}


}

/// @nodoc
abstract mixin class $DartThreadMessageStorePatch_ResetCopyWith<$Res> implements $DartThreadMessageStorePatchCopyWith<$Res> {
  factory $DartThreadMessageStorePatch_ResetCopyWith(DartThreadMessageStorePatch_Reset value, $Res Function(DartThreadMessageStorePatch_Reset) _then) = _$DartThreadMessageStorePatch_ResetCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, String threadKind, String threadId, DartConversationIdentity? conversationIdentity, List<DartMessage> items
});




}
/// @nodoc
class _$DartThreadMessageStorePatch_ResetCopyWithImpl<$Res>
    implements $DartThreadMessageStorePatch_ResetCopyWith<$Res> {
  _$DartThreadMessageStorePatch_ResetCopyWithImpl(this._self, this._then);

  final DartThreadMessageStorePatch_Reset _self;
  final $Res Function(DartThreadMessageStorePatch_Reset) _then;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? threadKind = null,Object? threadId = null,Object? conversationIdentity = freezed,Object? items = null,}) {
  return _then(DartThreadMessageStorePatch_Reset(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,threadKind: null == threadKind ? _self.threadKind : threadKind // ignore: cast_nullable_to_non_nullable
as String,threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,conversationIdentity: freezed == conversationIdentity ? _self.conversationIdentity : conversationIdentity // ignore: cast_nullable_to_non_nullable
as DartConversationIdentity?,items: null == items ? _self._items : items // ignore: cast_nullable_to_non_nullable
as List<DartMessage>,
  ));
}


}

/// @nodoc


class DartThreadMessageStorePatch_Upsert extends DartThreadMessageStorePatch {
  const DartThreadMessageStorePatch_Upsert({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.threadKind, required this.threadId, this.conversationIdentity, required this.message, required this.index}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  String threadKind;
@override final  String threadId;
@override final  DartConversationIdentity? conversationIdentity;
 final  DartMessage message;
 final  int index;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadMessageStorePatch_UpsertCopyWith<DartThreadMessageStorePatch_Upsert> get copyWith => _$DartThreadMessageStorePatch_UpsertCopyWithImpl<DartThreadMessageStorePatch_Upsert>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadMessageStorePatch_Upsert&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.threadKind, threadKind) || other.threadKind == threadKind)&&(identical(other.threadId, threadId) || other.threadId == threadId)&&(identical(other.conversationIdentity, conversationIdentity) || other.conversationIdentity == conversationIdentity)&&(identical(other.message, message) || other.message == message)&&(identical(other.index, index) || other.index == index));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,threadKind,threadId,conversationIdentity,message,index);

@override
String toString() {
  return 'DartThreadMessageStorePatch.upsert(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, threadKind: $threadKind, threadId: $threadId, conversationIdentity: $conversationIdentity, message: $message, index: $index)';
}


}

/// @nodoc
abstract mixin class $DartThreadMessageStorePatch_UpsertCopyWith<$Res> implements $DartThreadMessageStorePatchCopyWith<$Res> {
  factory $DartThreadMessageStorePatch_UpsertCopyWith(DartThreadMessageStorePatch_Upsert value, $Res Function(DartThreadMessageStorePatch_Upsert) _then) = _$DartThreadMessageStorePatch_UpsertCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, String threadKind, String threadId, DartConversationIdentity? conversationIdentity, DartMessage message, int index
});




}
/// @nodoc
class _$DartThreadMessageStorePatch_UpsertCopyWithImpl<$Res>
    implements $DartThreadMessageStorePatch_UpsertCopyWith<$Res> {
  _$DartThreadMessageStorePatch_UpsertCopyWithImpl(this._self, this._then);

  final DartThreadMessageStorePatch_Upsert _self;
  final $Res Function(DartThreadMessageStorePatch_Upsert) _then;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? threadKind = null,Object? threadId = null,Object? conversationIdentity = freezed,Object? message = null,Object? index = null,}) {
  return _then(DartThreadMessageStorePatch_Upsert(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,threadKind: null == threadKind ? _self.threadKind : threadKind // ignore: cast_nullable_to_non_nullable
as String,threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,conversationIdentity: freezed == conversationIdentity ? _self.conversationIdentity : conversationIdentity // ignore: cast_nullable_to_non_nullable
as DartConversationIdentity?,message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as DartMessage,index: null == index ? _self.index : index // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class DartThreadMessageStorePatch_Remove extends DartThreadMessageStorePatch {
  const DartThreadMessageStorePatch_Remove({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.threadKind, required this.threadId, this.conversationIdentity, required this.messageId}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  String threadKind;
@override final  String threadId;
@override final  DartConversationIdentity? conversationIdentity;
 final  String messageId;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadMessageStorePatch_RemoveCopyWith<DartThreadMessageStorePatch_Remove> get copyWith => _$DartThreadMessageStorePatch_RemoveCopyWithImpl<DartThreadMessageStorePatch_Remove>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadMessageStorePatch_Remove&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.threadKind, threadKind) || other.threadKind == threadKind)&&(identical(other.threadId, threadId) || other.threadId == threadId)&&(identical(other.conversationIdentity, conversationIdentity) || other.conversationIdentity == conversationIdentity)&&(identical(other.messageId, messageId) || other.messageId == messageId));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,threadKind,threadId,conversationIdentity,messageId);

@override
String toString() {
  return 'DartThreadMessageStorePatch.remove(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, threadKind: $threadKind, threadId: $threadId, conversationIdentity: $conversationIdentity, messageId: $messageId)';
}


}

/// @nodoc
abstract mixin class $DartThreadMessageStorePatch_RemoveCopyWith<$Res> implements $DartThreadMessageStorePatchCopyWith<$Res> {
  factory $DartThreadMessageStorePatch_RemoveCopyWith(DartThreadMessageStorePatch_Remove value, $Res Function(DartThreadMessageStorePatch_Remove) _then) = _$DartThreadMessageStorePatch_RemoveCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, String threadKind, String threadId, DartConversationIdentity? conversationIdentity, String messageId
});




}
/// @nodoc
class _$DartThreadMessageStorePatch_RemoveCopyWithImpl<$Res>
    implements $DartThreadMessageStorePatch_RemoveCopyWith<$Res> {
  _$DartThreadMessageStorePatch_RemoveCopyWithImpl(this._self, this._then);

  final DartThreadMessageStorePatch_Remove _self;
  final $Res Function(DartThreadMessageStorePatch_Remove) _then;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? threadKind = null,Object? threadId = null,Object? conversationIdentity = freezed,Object? messageId = null,}) {
  return _then(DartThreadMessageStorePatch_Remove(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,threadKind: null == threadKind ? _self.threadKind : threadKind // ignore: cast_nullable_to_non_nullable
as String,threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,conversationIdentity: freezed == conversationIdentity ? _self.conversationIdentity : conversationIdentity // ignore: cast_nullable_to_non_nullable
as DartConversationIdentity?,messageId: null == messageId ? _self.messageId : messageId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartThreadMessageStorePatch_RepairRequired extends DartThreadMessageStorePatch {
  const DartThreadMessageStorePatch_RepairRequired({required this.ownerIdentityId, required this.ownerDid, required this.version, required this.threadKind, required this.threadId, this.conversationIdentity, required this.reason}): super._();


@override final  String ownerIdentityId;
@override final  String ownerDid;
@override final  BigInt version;
@override final  String threadKind;
@override final  String threadId;
@override final  DartConversationIdentity? conversationIdentity;
 final  String reason;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartThreadMessageStorePatch_RepairRequiredCopyWith<DartThreadMessageStorePatch_RepairRequired> get copyWith => _$DartThreadMessageStorePatch_RepairRequiredCopyWithImpl<DartThreadMessageStorePatch_RepairRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartThreadMessageStorePatch_RepairRequired&&(identical(other.ownerIdentityId, ownerIdentityId) || other.ownerIdentityId == ownerIdentityId)&&(identical(other.ownerDid, ownerDid) || other.ownerDid == ownerDid)&&(identical(other.version, version) || other.version == version)&&(identical(other.threadKind, threadKind) || other.threadKind == threadKind)&&(identical(other.threadId, threadId) || other.threadId == threadId)&&(identical(other.conversationIdentity, conversationIdentity) || other.conversationIdentity == conversationIdentity)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,ownerIdentityId,ownerDid,version,threadKind,threadId,conversationIdentity,reason);

@override
String toString() {
  return 'DartThreadMessageStorePatch.repairRequired(ownerIdentityId: $ownerIdentityId, ownerDid: $ownerDid, version: $version, threadKind: $threadKind, threadId: $threadId, conversationIdentity: $conversationIdentity, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $DartThreadMessageStorePatch_RepairRequiredCopyWith<$Res> implements $DartThreadMessageStorePatchCopyWith<$Res> {
  factory $DartThreadMessageStorePatch_RepairRequiredCopyWith(DartThreadMessageStorePatch_RepairRequired value, $Res Function(DartThreadMessageStorePatch_RepairRequired) _then) = _$DartThreadMessageStorePatch_RepairRequiredCopyWithImpl;
@override @useResult
$Res call({
 String ownerIdentityId, String ownerDid, BigInt version, String threadKind, String threadId, DartConversationIdentity? conversationIdentity, String reason
});




}
/// @nodoc
class _$DartThreadMessageStorePatch_RepairRequiredCopyWithImpl<$Res>
    implements $DartThreadMessageStorePatch_RepairRequiredCopyWith<$Res> {
  _$DartThreadMessageStorePatch_RepairRequiredCopyWithImpl(this._self, this._then);

  final DartThreadMessageStorePatch_RepairRequired _self;
  final $Res Function(DartThreadMessageStorePatch_RepairRequired) _then;

/// Create a copy of DartThreadMessageStorePatch
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? ownerIdentityId = null,Object? ownerDid = null,Object? version = null,Object? threadKind = null,Object? threadId = null,Object? conversationIdentity = freezed,Object? reason = null,}) {
  return _then(DartThreadMessageStorePatch_RepairRequired(
ownerIdentityId: null == ownerIdentityId ? _self.ownerIdentityId : ownerIdentityId // ignore: cast_nullable_to_non_nullable
as String,ownerDid: null == ownerDid ? _self.ownerDid : ownerDid // ignore: cast_nullable_to_non_nullable
as String,version: null == version ? _self.version : version // ignore: cast_nullable_to_non_nullable
as BigInt,threadKind: null == threadKind ? _self.threadKind : threadKind // ignore: cast_nullable_to_non_nullable
as String,threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,conversationIdentity: freezed == conversationIdentity ? _self.conversationIdentity : conversationIdentity // ignore: cast_nullable_to_non_nullable
as DartConversationIdentity?,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
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
