// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'directory.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$DartIdentitySubject {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySubject);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartIdentitySubject()';
}


}

/// @nodoc
class $DartIdentitySubjectCopyWith<$Res>  {
$DartIdentitySubjectCopyWith(DartIdentitySubject _, $Res Function(DartIdentitySubject) __);
}


/// Adds pattern-matching-related methods to [DartIdentitySubject].
extension DartIdentitySubjectPatterns on DartIdentitySubject {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartIdentitySubject_Did value)?  did,TResult Function( DartIdentitySubject_Handle value)?  handle,TResult Function( DartIdentitySubject_Any value)?  any,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartIdentitySubject_Did() when did != null:
return did(_that);case DartIdentitySubject_Handle() when handle != null:
return handle(_that);case DartIdentitySubject_Any() when any != null:
return any(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartIdentitySubject_Did value)  did,required TResult Function( DartIdentitySubject_Handle value)  handle,required TResult Function( DartIdentitySubject_Any value)  any,}){
final _that = this;
switch (_that) {
case DartIdentitySubject_Did():
return did(_that);case DartIdentitySubject_Handle():
return handle(_that);case DartIdentitySubject_Any():
return any(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartIdentitySubject_Did value)?  did,TResult? Function( DartIdentitySubject_Handle value)?  handle,TResult? Function( DartIdentitySubject_Any value)?  any,}){
final _that = this;
switch (_that) {
case DartIdentitySubject_Did() when did != null:
return did(_that);case DartIdentitySubject_Handle() when handle != null:
return handle(_that);case DartIdentitySubject_Any() when any != null:
return any(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String did)?  did,TResult Function( String handle)?  handle,TResult Function( String value)?  any,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartIdentitySubject_Did() when did != null:
return did(_that.did);case DartIdentitySubject_Handle() when handle != null:
return handle(_that.handle);case DartIdentitySubject_Any() when any != null:
return any(_that.value);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String did)  did,required TResult Function( String handle)  handle,required TResult Function( String value)  any,}) {final _that = this;
switch (_that) {
case DartIdentitySubject_Did():
return did(_that.did);case DartIdentitySubject_Handle():
return handle(_that.handle);case DartIdentitySubject_Any():
return any(_that.value);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String did)?  did,TResult? Function( String handle)?  handle,TResult? Function( String value)?  any,}) {final _that = this;
switch (_that) {
case DartIdentitySubject_Did() when did != null:
return did(_that.did);case DartIdentitySubject_Handle() when handle != null:
return handle(_that.handle);case DartIdentitySubject_Any() when any != null:
return any(_that.value);case _:
  return null;

}
}

}

/// @nodoc


class DartIdentitySubject_Did extends DartIdentitySubject {
  const DartIdentitySubject_Did({required this.did}): super._();


 final  String did;

/// Create a copy of DartIdentitySubject
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySubject_DidCopyWith<DartIdentitySubject_Did> get copyWith => _$DartIdentitySubject_DidCopyWithImpl<DartIdentitySubject_Did>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySubject_Did&&(identical(other.did, did) || other.did == did));
}


@override
int get hashCode => Object.hash(runtimeType,did);

@override
String toString() {
  return 'DartIdentitySubject.did(did: $did)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySubject_DidCopyWith<$Res> implements $DartIdentitySubjectCopyWith<$Res> {
  factory $DartIdentitySubject_DidCopyWith(DartIdentitySubject_Did value, $Res Function(DartIdentitySubject_Did) _then) = _$DartIdentitySubject_DidCopyWithImpl;
@useResult
$Res call({
 String did
});




}
/// @nodoc
class _$DartIdentitySubject_DidCopyWithImpl<$Res>
    implements $DartIdentitySubject_DidCopyWith<$Res> {
  _$DartIdentitySubject_DidCopyWithImpl(this._self, this._then);

  final DartIdentitySubject_Did _self;
  final $Res Function(DartIdentitySubject_Did) _then;

/// Create a copy of DartIdentitySubject
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? did = null,}) {
  return _then(DartIdentitySubject_Did(
did: null == did ? _self.did : did // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartIdentitySubject_Handle extends DartIdentitySubject {
  const DartIdentitySubject_Handle({required this.handle}): super._();


 final  String handle;

/// Create a copy of DartIdentitySubject
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySubject_HandleCopyWith<DartIdentitySubject_Handle> get copyWith => _$DartIdentitySubject_HandleCopyWithImpl<DartIdentitySubject_Handle>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySubject_Handle&&(identical(other.handle, handle) || other.handle == handle));
}


@override
int get hashCode => Object.hash(runtimeType,handle);

@override
String toString() {
  return 'DartIdentitySubject.handle(handle: $handle)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySubject_HandleCopyWith<$Res> implements $DartIdentitySubjectCopyWith<$Res> {
  factory $DartIdentitySubject_HandleCopyWith(DartIdentitySubject_Handle value, $Res Function(DartIdentitySubject_Handle) _then) = _$DartIdentitySubject_HandleCopyWithImpl;
@useResult
$Res call({
 String handle
});




}
/// @nodoc
class _$DartIdentitySubject_HandleCopyWithImpl<$Res>
    implements $DartIdentitySubject_HandleCopyWith<$Res> {
  _$DartIdentitySubject_HandleCopyWithImpl(this._self, this._then);

  final DartIdentitySubject_Handle _self;
  final $Res Function(DartIdentitySubject_Handle) _then;

/// Create a copy of DartIdentitySubject
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? handle = null,}) {
  return _then(DartIdentitySubject_Handle(
handle: null == handle ? _self.handle : handle // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartIdentitySubject_Any extends DartIdentitySubject {
  const DartIdentitySubject_Any({required this.value}): super._();


 final  String value;

/// Create a copy of DartIdentitySubject
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartIdentitySubject_AnyCopyWith<DartIdentitySubject_Any> get copyWith => _$DartIdentitySubject_AnyCopyWithImpl<DartIdentitySubject_Any>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartIdentitySubject_Any&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode => Object.hash(runtimeType,value);

@override
String toString() {
  return 'DartIdentitySubject.any(value: $value)';
}


}

/// @nodoc
abstract mixin class $DartIdentitySubject_AnyCopyWith<$Res> implements $DartIdentitySubjectCopyWith<$Res> {
  factory $DartIdentitySubject_AnyCopyWith(DartIdentitySubject_Any value, $Res Function(DartIdentitySubject_Any) _then) = _$DartIdentitySubject_AnyCopyWithImpl;
@useResult
$Res call({
 String value
});




}
/// @nodoc
class _$DartIdentitySubject_AnyCopyWithImpl<$Res>
    implements $DartIdentitySubject_AnyCopyWith<$Res> {
  _$DartIdentitySubject_AnyCopyWithImpl(this._self, this._then);

  final DartIdentitySubject_Any _self;
  final $Res Function(DartIdentitySubject_Any) _then;

/// Create a copy of DartIdentitySubject
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? value = null,}) {
  return _then(DartIdentitySubject_Any(
value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on
