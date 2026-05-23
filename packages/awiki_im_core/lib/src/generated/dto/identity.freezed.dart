// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'identity.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$DartIdentitySelector {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySelector);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartIdentitySelector()';
}


}

/// @nodoc
class $DartIdentitySelectorCopyWith<$Res>  {
$DartIdentitySelectorCopyWith(DartIdentitySelector _, $Res Function(DartIdentitySelector) __);
}


/// Adds pattern-matching-related methods to [DartIdentitySelector].
extension DartIdentitySelectorPatterns on DartIdentitySelector {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartIdentitySelector_Default value)?  default_,TResult Function( DartIdentitySelector_Id value)?  id,TResult Function( DartIdentitySelector_Did value)?  did,TResult Function( DartIdentitySelector_Handle value)?  handle,TResult Function( DartIdentitySelector_LocalAlias value)?  localAlias,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartIdentitySelector_Default() when default_ != null:
return default_(_that);case DartIdentitySelector_Id() when id != null:
return id(_that);case DartIdentitySelector_Did() when did != null:
return did(_that);case DartIdentitySelector_Handle() when handle != null:
return handle(_that);case DartIdentitySelector_LocalAlias() when localAlias != null:
return localAlias(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartIdentitySelector_Default value)  default_,required TResult Function( DartIdentitySelector_Id value)  id,required TResult Function( DartIdentitySelector_Did value)  did,required TResult Function( DartIdentitySelector_Handle value)  handle,required TResult Function( DartIdentitySelector_LocalAlias value)  localAlias,}){
final _that = this;
switch (_that) {
case DartIdentitySelector_Default():
return default_(_that);case DartIdentitySelector_Id():
return id(_that);case DartIdentitySelector_Did():
return did(_that);case DartIdentitySelector_Handle():
return handle(_that);case DartIdentitySelector_LocalAlias():
return localAlias(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartIdentitySelector_Default value)?  default_,TResult? Function( DartIdentitySelector_Id value)?  id,TResult? Function( DartIdentitySelector_Did value)?  did,TResult? Function( DartIdentitySelector_Handle value)?  handle,TResult? Function( DartIdentitySelector_LocalAlias value)?  localAlias,}){
final _that = this;
switch (_that) {
case DartIdentitySelector_Default() when default_ != null:
return default_(_that);case DartIdentitySelector_Id() when id != null:
return id(_that);case DartIdentitySelector_Did() when did != null:
return did(_that);case DartIdentitySelector_Handle() when handle != null:
return handle(_that);case DartIdentitySelector_LocalAlias() when localAlias != null:
return localAlias(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  default_,TResult Function( String id)?  id,TResult Function( String did)?  did,TResult Function( String handle)?  handle,TResult Function( String alias)?  localAlias,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartIdentitySelector_Default() when default_ != null:
return default_();case DartIdentitySelector_Id() when id != null:
return id(_that.id);case DartIdentitySelector_Did() when did != null:
return did(_that.did);case DartIdentitySelector_Handle() when handle != null:
return handle(_that.handle);case DartIdentitySelector_LocalAlias() when localAlias != null:
return localAlias(_that.alias);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  default_,required TResult Function( String id)  id,required TResult Function( String did)  did,required TResult Function( String handle)  handle,required TResult Function( String alias)  localAlias,}) {final _that = this;
switch (_that) {
case DartIdentitySelector_Default():
return default_();case DartIdentitySelector_Id():
return id(_that.id);case DartIdentitySelector_Did():
return did(_that.did);case DartIdentitySelector_Handle():
return handle(_that.handle);case DartIdentitySelector_LocalAlias():
return localAlias(_that.alias);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  default_,TResult? Function( String id)?  id,TResult? Function( String did)?  did,TResult? Function( String handle)?  handle,TResult? Function( String alias)?  localAlias,}) {final _that = this;
switch (_that) {
case DartIdentitySelector_Default() when default_ != null:
return default_();case DartIdentitySelector_Id() when id != null:
return id(_that.id);case DartIdentitySelector_Did() when did != null:
return did(_that.did);case DartIdentitySelector_Handle() when handle != null:
return handle(_that.handle);case DartIdentitySelector_LocalAlias() when localAlias != null:
return localAlias(_that.alias);case _:
  return null;

}
}

}

/// @nodoc


class DartIdentitySelector_Default extends DartIdentitySelector {
  const DartIdentitySelector_Default(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySelector_Default);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartIdentitySelector.default_()';
}


}




/// @nodoc


class DartIdentitySelector_Id extends DartIdentitySelector {
  const DartIdentitySelector_Id({required this.id}): super._();


 final  String id;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySelector_IdCopyWith<DartIdentitySelector_Id> get copyWith => _$DartIdentitySelector_IdCopyWithImpl<DartIdentitySelector_Id>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySelector_Id&&(identical(other.id, id) || other.id == id));
}


@override
int get hashCode => Object.hash(runtimeType,id);

@override
String toString() {
  return 'DartIdentitySelector.id(id: $id)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySelector_IdCopyWith<$Res> implements $DartIdentitySelectorCopyWith<$Res> {
  factory $DartIdentitySelector_IdCopyWith(DartIdentitySelector_Id value, $Res Function(DartIdentitySelector_Id) _then) = _$DartIdentitySelector_IdCopyWithImpl;
@useResult
$Res call({
 String id
});




}
/// @nodoc
class _$DartIdentitySelector_IdCopyWithImpl<$Res>
    implements $DartIdentitySelector_IdCopyWith<$Res> {
  _$DartIdentitySelector_IdCopyWithImpl(this._self, this._then);

  final DartIdentitySelector_Id _self;
  final $Res Function(DartIdentitySelector_Id) _then;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? id = null,}) {
  return _then(DartIdentitySelector_Id(
id: null == id ? _self.id : id // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartIdentitySelector_Did extends DartIdentitySelector {
  const DartIdentitySelector_Did({required this.did}): super._();


 final  String did;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySelector_DidCopyWith<DartIdentitySelector_Did> get copyWith => _$DartIdentitySelector_DidCopyWithImpl<DartIdentitySelector_Did>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySelector_Did&&(identical(other.did, did) || other.did == did));
}


@override
int get hashCode => Object.hash(runtimeType,did);

@override
String toString() {
  return 'DartIdentitySelector.did(did: $did)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySelector_DidCopyWith<$Res> implements $DartIdentitySelectorCopyWith<$Res> {
  factory $DartIdentitySelector_DidCopyWith(DartIdentitySelector_Did value, $Res Function(DartIdentitySelector_Did) _then) = _$DartIdentitySelector_DidCopyWithImpl;
@useResult
$Res call({
 String did
});




}
/// @nodoc
class _$DartIdentitySelector_DidCopyWithImpl<$Res>
    implements $DartIdentitySelector_DidCopyWith<$Res> {
  _$DartIdentitySelector_DidCopyWithImpl(this._self, this._then);

  final DartIdentitySelector_Did _self;
  final $Res Function(DartIdentitySelector_Did) _then;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? did = null,}) {
  return _then(DartIdentitySelector_Did(
did: null == did ? _self.did : did // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartIdentitySelector_Handle extends DartIdentitySelector {
  const DartIdentitySelector_Handle({required this.handle}): super._();


 final  String handle;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySelector_HandleCopyWith<DartIdentitySelector_Handle> get copyWith => _$DartIdentitySelector_HandleCopyWithImpl<DartIdentitySelector_Handle>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySelector_Handle&&(identical(other.handle, handle) || other.handle == handle));
}


@override
int get hashCode => Object.hash(runtimeType,handle);

@override
String toString() {
  return 'DartIdentitySelector.handle(handle: $handle)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySelector_HandleCopyWith<$Res> implements $DartIdentitySelectorCopyWith<$Res> {
  factory $DartIdentitySelector_HandleCopyWith(DartIdentitySelector_Handle value, $Res Function(DartIdentitySelector_Handle) _then) = _$DartIdentitySelector_HandleCopyWithImpl;
@useResult
$Res call({
 String handle
});




}
/// @nodoc
class _$DartIdentitySelector_HandleCopyWithImpl<$Res>
    implements $DartIdentitySelector_HandleCopyWith<$Res> {
  _$DartIdentitySelector_HandleCopyWithImpl(this._self, this._then);

  final DartIdentitySelector_Handle _self;
  final $Res Function(DartIdentitySelector_Handle) _then;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? handle = null,}) {
  return _then(DartIdentitySelector_Handle(
handle: null == handle ? _self.handle : handle // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartIdentitySelector_LocalAlias extends DartIdentitySelector {
  const DartIdentitySelector_LocalAlias({required this.alias}): super._();


 final  String alias;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySelector_LocalAliasCopyWith<DartIdentitySelector_LocalAlias> get copyWith => _$DartIdentitySelector_LocalAliasCopyWithImpl<DartIdentitySelector_LocalAlias>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySelector_LocalAlias&&(identical(other.alias, alias) || other.alias == alias));
}


@override
int get hashCode => Object.hash(runtimeType,alias);

@override
String toString() {
  return 'DartIdentitySelector.localAlias(alias: $alias)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySelector_LocalAliasCopyWith<$Res> implements $DartIdentitySelectorCopyWith<$Res> {
  factory $DartIdentitySelector_LocalAliasCopyWith(DartIdentitySelector_LocalAlias value, $Res Function(DartIdentitySelector_LocalAlias) _then) = _$DartIdentitySelector_LocalAliasCopyWithImpl;
@useResult
$Res call({
 String alias
});




}
/// @nodoc
class _$DartIdentitySelector_LocalAliasCopyWithImpl<$Res>
    implements $DartIdentitySelector_LocalAliasCopyWith<$Res> {
  _$DartIdentitySelector_LocalAliasCopyWithImpl(this._self, this._then);

  final DartIdentitySelector_LocalAlias _self;
  final $Res Function(DartIdentitySelector_LocalAlias) _then;

/// Create a copy of DartIdentitySelector
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? alias = null,}) {
  return _then(DartIdentitySelector_LocalAlias(
alias: null == alias ? _self.alias : alias // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on
