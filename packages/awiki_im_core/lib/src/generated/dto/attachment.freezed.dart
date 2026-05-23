// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'attachment.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$DartAttachmentDestination {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartAttachmentDestination);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartAttachmentDestination()';
}


}

/// @nodoc
class $DartAttachmentDestinationCopyWith<$Res>  {
$DartAttachmentDestinationCopyWith(DartAttachmentDestination _, $Res Function(DartAttachmentDestination) __);
}


/// Adds pattern-matching-related methods to [DartAttachmentDestination].
extension DartAttachmentDestinationPatterns on DartAttachmentDestination {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartAttachmentDestination_LocalFile value)?  localFile,TResult Function( DartAttachmentDestination_Memory value)?  memory,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that);case DartAttachmentDestination_Memory() when memory != null:
return memory(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartAttachmentDestination_LocalFile value)  localFile,required TResult Function( DartAttachmentDestination_Memory value)  memory,}){
final _that = this;
switch (_that) {
case DartAttachmentDestination_LocalFile():
return localFile(_that);case DartAttachmentDestination_Memory():
return memory(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartAttachmentDestination_LocalFile value)?  localFile,TResult? Function( DartAttachmentDestination_Memory value)?  memory,}){
final _that = this;
switch (_that) {
case DartAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that);case DartAttachmentDestination_Memory() when memory != null:
return memory(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String path)?  localFile,TResult Function()?  memory,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that.path);case DartAttachmentDestination_Memory() when memory != null:
return memory();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String path)  localFile,required TResult Function()  memory,}) {final _that = this;
switch (_that) {
case DartAttachmentDestination_LocalFile():
return localFile(_that.path);case DartAttachmentDestination_Memory():
return memory();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String path)?  localFile,TResult? Function()?  memory,}) {final _that = this;
switch (_that) {
case DartAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that.path);case DartAttachmentDestination_Memory() when memory != null:
return memory();case _:
  return null;

}
}

}

/// @nodoc


class DartAttachmentDestination_LocalFile extends DartAttachmentDestination {
  const DartAttachmentDestination_LocalFile({required this.path}): super._();


 final  String path;

/// Create a copy of DartAttachmentDestination
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartAttachmentDestination_LocalFileCopyWith<DartAttachmentDestination_LocalFile> get copyWith => _$DartAttachmentDestination_LocalFileCopyWithImpl<DartAttachmentDestination_LocalFile>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartAttachmentDestination_LocalFile&&(identical(other.path, path) || other.path == path));
}


@override
int get hashCode => Object.hash(runtimeType,path);

@override
String toString() {
  return 'DartAttachmentDestination.localFile(path: $path)';
}


}

/// @nodoc
abstract mixin class $DartAttachmentDestination_LocalFileCopyWith<$Res> implements $DartAttachmentDestinationCopyWith<$Res> {
  factory $DartAttachmentDestination_LocalFileCopyWith(DartAttachmentDestination_LocalFile value, $Res Function(DartAttachmentDestination_LocalFile) _then) = _$DartAttachmentDestination_LocalFileCopyWithImpl;
@useResult
$Res call({
 String path
});




}
/// @nodoc
class _$DartAttachmentDestination_LocalFileCopyWithImpl<$Res>
    implements $DartAttachmentDestination_LocalFileCopyWith<$Res> {
  _$DartAttachmentDestination_LocalFileCopyWithImpl(this._self, this._then);

  final DartAttachmentDestination_LocalFile _self;
  final $Res Function(DartAttachmentDestination_LocalFile) _then;

/// Create a copy of DartAttachmentDestination
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,}) {
  return _then(DartAttachmentDestination_LocalFile(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartAttachmentDestination_Memory extends DartAttachmentDestination {
  const DartAttachmentDestination_Memory(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartAttachmentDestination_Memory);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartAttachmentDestination.memory()';
}


}




/// @nodoc
mixin _$DartAttachmentInput {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartAttachmentInput);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartAttachmentInput()';
}


}

/// @nodoc
class $DartAttachmentInputCopyWith<$Res>  {
$DartAttachmentInputCopyWith(DartAttachmentInput _, $Res Function(DartAttachmentInput) __);
}


/// Adds pattern-matching-related methods to [DartAttachmentInput].
extension DartAttachmentInputPatterns on DartAttachmentInput {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartAttachmentInput_LocalFile value)?  localFile,TResult Function( DartAttachmentInput_Bytes value)?  bytes,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartAttachmentInput_LocalFile() when localFile != null:
return localFile(_that);case DartAttachmentInput_Bytes() when bytes != null:
return bytes(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartAttachmentInput_LocalFile value)  localFile,required TResult Function( DartAttachmentInput_Bytes value)  bytes,}){
final _that = this;
switch (_that) {
case DartAttachmentInput_LocalFile():
return localFile(_that);case DartAttachmentInput_Bytes():
return bytes(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartAttachmentInput_LocalFile value)?  localFile,TResult? Function( DartAttachmentInput_Bytes value)?  bytes,}){
final _that = this;
switch (_that) {
case DartAttachmentInput_LocalFile() when localFile != null:
return localFile(_that);case DartAttachmentInput_Bytes() when bytes != null:
return bytes(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String path)?  localFile,TResult Function( String? filename,  String? mimeType,  Uint8List bytes)?  bytes,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartAttachmentInput_LocalFile() when localFile != null:
return localFile(_that.path);case DartAttachmentInput_Bytes() when bytes != null:
return bytes(_that.filename,_that.mimeType,_that.bytes);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String path)  localFile,required TResult Function( String? filename,  String? mimeType,  Uint8List bytes)  bytes,}) {final _that = this;
switch (_that) {
case DartAttachmentInput_LocalFile():
return localFile(_that.path);case DartAttachmentInput_Bytes():
return bytes(_that.filename,_that.mimeType,_that.bytes);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String path)?  localFile,TResult? Function( String? filename,  String? mimeType,  Uint8List bytes)?  bytes,}) {final _that = this;
switch (_that) {
case DartAttachmentInput_LocalFile() when localFile != null:
return localFile(_that.path);case DartAttachmentInput_Bytes() when bytes != null:
return bytes(_that.filename,_that.mimeType,_that.bytes);case _:
  return null;

}
}

}

/// @nodoc


class DartAttachmentInput_LocalFile extends DartAttachmentInput {
  const DartAttachmentInput_LocalFile({required this.path}): super._();


 final  String path;

/// Create a copy of DartAttachmentInput
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartAttachmentInput_LocalFileCopyWith<DartAttachmentInput_LocalFile> get copyWith => _$DartAttachmentInput_LocalFileCopyWithImpl<DartAttachmentInput_LocalFile>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartAttachmentInput_LocalFile&&(identical(other.path, path) || other.path == path));
}


@override
int get hashCode => Object.hash(runtimeType,path);

@override
String toString() {
  return 'DartAttachmentInput.localFile(path: $path)';
}


}

/// @nodoc
abstract mixin class $DartAttachmentInput_LocalFileCopyWith<$Res> implements $DartAttachmentInputCopyWith<$Res> {
  factory $DartAttachmentInput_LocalFileCopyWith(DartAttachmentInput_LocalFile value, $Res Function(DartAttachmentInput_LocalFile) _then) = _$DartAttachmentInput_LocalFileCopyWithImpl;
@useResult
$Res call({
 String path
});




}
/// @nodoc
class _$DartAttachmentInput_LocalFileCopyWithImpl<$Res>
    implements $DartAttachmentInput_LocalFileCopyWith<$Res> {
  _$DartAttachmentInput_LocalFileCopyWithImpl(this._self, this._then);

  final DartAttachmentInput_LocalFile _self;
  final $Res Function(DartAttachmentInput_LocalFile) _then;

/// Create a copy of DartAttachmentInput
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,}) {
  return _then(DartAttachmentInput_LocalFile(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartAttachmentInput_Bytes extends DartAttachmentInput {
  const DartAttachmentInput_Bytes({this.filename, this.mimeType, required this.bytes}): super._();


 final  String? filename;
 final  String? mimeType;
 final  Uint8List bytes;

/// Create a copy of DartAttachmentInput
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartAttachmentInput_BytesCopyWith<DartAttachmentInput_Bytes> get copyWith => _$DartAttachmentInput_BytesCopyWithImpl<DartAttachmentInput_Bytes>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartAttachmentInput_Bytes&&(identical(other.filename, filename) || other.filename == filename)&&(identical(other.mimeType, mimeType) || other.mimeType == mimeType)&&const DeepCollectionEquality().equals(other.bytes, bytes));
}


@override
int get hashCode => Object.hash(runtimeType,filename,mimeType,const DeepCollectionEquality().hash(bytes));

@override
String toString() {
  return 'DartAttachmentInput.bytes(filename: $filename, mimeType: $mimeType, bytes: $bytes)';
}


}

/// @nodoc
abstract mixin class $DartAttachmentInput_BytesCopyWith<$Res> implements $DartAttachmentInputCopyWith<$Res> {
  factory $DartAttachmentInput_BytesCopyWith(DartAttachmentInput_Bytes value, $Res Function(DartAttachmentInput_Bytes) _then) = _$DartAttachmentInput_BytesCopyWithImpl;
@useResult
$Res call({
 String? filename, String? mimeType, Uint8List bytes
});




}
/// @nodoc
class _$DartAttachmentInput_BytesCopyWithImpl<$Res>
    implements $DartAttachmentInput_BytesCopyWith<$Res> {
  _$DartAttachmentInput_BytesCopyWithImpl(this._self, this._then);

  final DartAttachmentInput_Bytes _self;
  final $Res Function(DartAttachmentInput_Bytes) _then;

/// Create a copy of DartAttachmentInput
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? filename = freezed,Object? mimeType = freezed,Object? bytes = null,}) {
  return _then(DartAttachmentInput_Bytes(
filename: freezed == filename ? _self.filename : filename // ignore: cast_nullable_to_non_nullable
as String?,mimeType: freezed == mimeType ? _self.mimeType : mimeType // ignore: cast_nullable_to_non_nullable
as String?,bytes: null == bytes ? _self.bytes : bytes // ignore: cast_nullable_to_non_nullable
as Uint8List,
  ));
}


}

/// @nodoc
mixin _$DartDownloadedAttachmentDestination {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartDownloadedAttachmentDestination);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'DartDownloadedAttachmentDestination()';
}


}

/// @nodoc
class $DartDownloadedAttachmentDestinationCopyWith<$Res>  {
$DartDownloadedAttachmentDestinationCopyWith(DartDownloadedAttachmentDestination _, $Res Function(DartDownloadedAttachmentDestination) __);
}


/// Adds pattern-matching-related methods to [DartDownloadedAttachmentDestination].
extension DartDownloadedAttachmentDestinationPatterns on DartDownloadedAttachmentDestination {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( DartDownloadedAttachmentDestination_LocalFile value)?  localFile,TResult Function( DartDownloadedAttachmentDestination_Memory value)?  memory,required TResult orElse(),}){
final _that = this;
switch (_that) {
case DartDownloadedAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that);case DartDownloadedAttachmentDestination_Memory() when memory != null:
return memory(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( DartDownloadedAttachmentDestination_LocalFile value)  localFile,required TResult Function( DartDownloadedAttachmentDestination_Memory value)  memory,}){
final _that = this;
switch (_that) {
case DartDownloadedAttachmentDestination_LocalFile():
return localFile(_that);case DartDownloadedAttachmentDestination_Memory():
return memory(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( DartDownloadedAttachmentDestination_LocalFile value)?  localFile,TResult? Function( DartDownloadedAttachmentDestination_Memory value)?  memory,}){
final _that = this;
switch (_that) {
case DartDownloadedAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that);case DartDownloadedAttachmentDestination_Memory() when memory != null:
return memory(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String path)?  localFile,TResult Function( Uint8List bytes)?  memory,required TResult orElse(),}) {final _that = this;
switch (_that) {
case DartDownloadedAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that.path);case DartDownloadedAttachmentDestination_Memory() when memory != null:
return memory(_that.bytes);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String path)  localFile,required TResult Function( Uint8List bytes)  memory,}) {final _that = this;
switch (_that) {
case DartDownloadedAttachmentDestination_LocalFile():
return localFile(_that.path);case DartDownloadedAttachmentDestination_Memory():
return memory(_that.bytes);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String path)?  localFile,TResult? Function( Uint8List bytes)?  memory,}) {final _that = this;
switch (_that) {
case DartDownloadedAttachmentDestination_LocalFile() when localFile != null:
return localFile(_that.path);case DartDownloadedAttachmentDestination_Memory() when memory != null:
return memory(_that.bytes);case _:
  return null;

}
}

}

/// @nodoc


class DartDownloadedAttachmentDestination_LocalFile extends DartDownloadedAttachmentDestination {
  const DartDownloadedAttachmentDestination_LocalFile({required this.path}): super._();


 final  String path;

/// Create a copy of DartDownloadedAttachmentDestination
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartDownloadedAttachmentDestination_LocalFileCopyWith<DartDownloadedAttachmentDestination_LocalFile> get copyWith => _$DartDownloadedAttachmentDestination_LocalFileCopyWithImpl<DartDownloadedAttachmentDestination_LocalFile>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartDownloadedAttachmentDestination_LocalFile&&(identical(other.path, path) || other.path == path));
}


@override
int get hashCode => Object.hash(runtimeType,path);

@override
String toString() {
  return 'DartDownloadedAttachmentDestination.localFile(path: $path)';
}


}

/// @nodoc
abstract mixin class $DartDownloadedAttachmentDestination_LocalFileCopyWith<$Res> implements $DartDownloadedAttachmentDestinationCopyWith<$Res> {
  factory $DartDownloadedAttachmentDestination_LocalFileCopyWith(DartDownloadedAttachmentDestination_LocalFile value, $Res Function(DartDownloadedAttachmentDestination_LocalFile) _then) = _$DartDownloadedAttachmentDestination_LocalFileCopyWithImpl;
@useResult
$Res call({
 String path
});




}
/// @nodoc
class _$DartDownloadedAttachmentDestination_LocalFileCopyWithImpl<$Res>
    implements $DartDownloadedAttachmentDestination_LocalFileCopyWith<$Res> {
  _$DartDownloadedAttachmentDestination_LocalFileCopyWithImpl(this._self, this._then);

  final DartDownloadedAttachmentDestination_LocalFile _self;
  final $Res Function(DartDownloadedAttachmentDestination_LocalFile) _then;

/// Create a copy of DartDownloadedAttachmentDestination
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,}) {
  return _then(DartDownloadedAttachmentDestination_LocalFile(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class DartDownloadedAttachmentDestination_Memory extends DartDownloadedAttachmentDestination {
  const DartDownloadedAttachmentDestination_Memory({required this.bytes}): super._();


 final  Uint8List bytes;

/// Create a copy of DartDownloadedAttachmentDestination
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$DartDownloadedAttachmentDestination_MemoryCopyWith<DartDownloadedAttachmentDestination_Memory> get copyWith => _$DartDownloadedAttachmentDestination_MemoryCopyWithImpl<DartDownloadedAttachmentDestination_Memory>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is DartDownloadedAttachmentDestination_Memory&&const DeepCollectionEquality().equals(other.bytes, bytes));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(bytes));

@override
String toString() {
  return 'DartDownloadedAttachmentDestination.memory(bytes: $bytes)';
}


}

/// @nodoc
abstract mixin class $DartDownloadedAttachmentDestination_MemoryCopyWith<$Res> implements $DartDownloadedAttachmentDestinationCopyWith<$Res> {
  factory $DartDownloadedAttachmentDestination_MemoryCopyWith(DartDownloadedAttachmentDestination_Memory value, $Res Function(DartDownloadedAttachmentDestination_Memory) _then) = _$DartDownloadedAttachmentDestination_MemoryCopyWithImpl;
@useResult
$Res call({
 Uint8List bytes
});




}
/// @nodoc
class _$DartDownloadedAttachmentDestination_MemoryCopyWithImpl<$Res>
    implements $DartDownloadedAttachmentDestination_MemoryCopyWith<$Res> {
  _$DartDownloadedAttachmentDestination_MemoryCopyWithImpl(this._self, this._then);

  final DartDownloadedAttachmentDestination_Memory _self;
  final $Res Function(DartDownloadedAttachmentDestination_Memory) _then;

/// Create a copy of DartDownloadedAttachmentDestination
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? bytes = null,}) {
  return _then(DartDownloadedAttachmentDestination_Memory(
bytes: null == bytes ? _self.bytes : bytes // ignore: cast_nullable_to_non_nullable
as Uint8List,
  ));
}


}

// dart format on
