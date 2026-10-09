// Duktape 2.3's built-in members, so feature tests see what they see under FMD2 (a script that
// checks `typeof Symbol` or `Array.prototype.find` takes Duktape's branch). Every own property
// of these objects that Duktape lacks is deleted; symbol-keyed properties stay, since QuickJS's
// operators read them (Date's @@toPrimitive, RegExp's @@replace). Members Duktape has and QuickJS
// lacks are in docs/duktape-differences.md.
//
// The lists are the own property names Duktape 2.3.0 reports (Object.getOwnPropertyNames), read
// from a build of it (crates/fmd-duktape-ref) with tests/duktape_reference.rs's inventory script.
(function () {
  'use strict';
  var global = globalThis;
  var members = {
    'global':
      'Array ArrayBuffer Boolean Buffer DataView Date Duktape Error EvalError Float32Array ' +
      'Float64Array Function Infinity Int16Array Int32Array Int8Array JSON Math NaN Number ' +
      'Object Proxy RangeError ReferenceError Reflect RegExp String SyntaxError TextDecoder ' +
      'TextEncoder TypeError URIError Uint16Array Uint32Array Uint8Array Uint8ClampedArray ' +
      'decodeURI decodeURIComponent encodeURI encodeURIComponent escape eval isFinite isNaN ' +
      'parseFloat parseInt performance print require undefined unescape',
    'Array': 'isArray length name prototype',
    'Array.prototype':
      'concat constructor every filter forEach indexOf join lastIndexOf length map pop push ' +
      'reduce reduceRight reverse shift slice some sort splice toLocaleString toString unshift',
    'ArrayBuffer': 'isView length name prototype',
    'ArrayBuffer.prototype': 'byteLength constructor slice',
    'Boolean': 'length name prototype',
    'Boolean.prototype': 'constructor toString valueOf',
    'Buffer': 'byteLength compare concat isBuffer isEncoding length name prototype',
    'Buffer.prototype':
      'compare constructor copy equals fill readDoubleBE readDoubleLE readFloatBE readFloatLE ' +
      'readInt16BE readInt16LE readInt32BE readInt32LE readInt8 readIntBE readIntLE ' +
      'readUInt16BE readUInt16LE readUInt32BE readUInt32LE readUInt8 readUIntBE readUIntLE ' +
      'slice toJSON toString write writeDoubleBE writeDoubleLE writeFloatBE writeFloatLE ' +
      'writeInt16BE writeInt16LE writeInt32BE writeInt32LE writeInt8 writeIntBE writeIntLE ' +
      'writeUInt16BE writeUInt16LE writeUInt32BE writeUInt32LE writeUInt8 writeUIntBE ' +
      'writeUIntLE',
    'DataView': 'length name prototype',
    'DataView.prototype':
      'buffer byteLength byteOffset constructor getFloat32 getFloat64 getInt16 getInt32 getInt8 ' +
      'getUint16 getUint32 getUint8 setFloat32 setFloat64 setInt16 setInt32 setInt8 setUint16 ' +
      'setUint32 setUint8',
    'Date': 'UTC length name now parse prototype',
    'Date.prototype':
      'constructor getDate getDay getFullYear getHours getMilliseconds getMinutes getMonth ' +
      'getSeconds getTime getTimezoneOffset getUTCDate getUTCDay getUTCFullYear getUTCHours ' +
      'getUTCMilliseconds getUTCMinutes getUTCMonth getUTCSeconds getYear setDate setFullYear ' +
      'setHours setMilliseconds setMinutes setMonth setSeconds setTime setUTCDate ' +
      'setUTCFullYear setUTCHours setUTCMilliseconds setUTCMinutes setUTCMonth setUTCSeconds ' +
      'setYear toDateString toGMTString toISOString toJSON toLocaleDateString toLocaleString ' +
      'toLocaleTimeString toString toTimeString toUTCString valueOf',
    'Duktape': 'Pointer Thread act compact dec enc env fin gc info modLoaded modSearch version',
    'Error': 'length name prototype',
    'Error.prototype': 'constructor fileName lineNumber message name stack toString',
    'EvalError': 'length name prototype',
    'EvalError.prototype': 'constructor message name',
    'Float32Array': 'BYTES_PER_ELEMENT length name prototype',
    'Float32Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Float64Array': 'BYTES_PER_ELEMENT length name prototype',
    'Float64Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Function': 'length name prototype',
    'Function.prototype': 'apply bind call constructor length name toString',
    'Int16Array': 'BYTES_PER_ELEMENT length name prototype',
    'Int16Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Int32Array': 'BYTES_PER_ELEMENT length name prototype',
    'Int32Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Int8Array': 'BYTES_PER_ELEMENT length name prototype',
    'Int8Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'JSON': 'parse stringify',
    'Math':
      'E LN10 LN2 LOG10E LOG2E PI SQRT1_2 SQRT2 abs acos asin atan atan2 cbrt ceil clz32 cos ' +
      'exp floor hypot imul log log10 log2 max min pow random round sign sin sqrt tan trunc',
    'Number':
      'EPSILON MAX_SAFE_INTEGER MAX_VALUE MIN_SAFE_INTEGER MIN_VALUE NEGATIVE_INFINITY NaN ' +
      'POSITIVE_INFINITY isFinite isInteger isNaN isSafeInteger length name parseFloat parseInt ' +
      'prototype',
    'Number.prototype':
      'constructor toExponential toFixed toLocaleString toPrecision toString valueOf',
    'Object':
      'assign create defineProperties defineProperty freeze getOwnPropertyDescriptor ' +
      'getOwnPropertyNames getOwnPropertySymbols getPrototypeOf is isExtensible isFrozen ' +
      'isSealed keys length name preventExtensions prototype seal setPrototypeOf',
    'Object.prototype':
      '__defineGetter__ __defineSetter__ __lookupGetter__ __lookupSetter__ __proto__ ' +
      'constructor hasOwnProperty isPrototypeOf propertyIsEnumerable toLocaleString toString ' +
      'valueOf',
    'Proxy': 'length name',
    'RangeError': 'length name prototype',
    'RangeError.prototype': 'constructor message name',
    'ReferenceError': 'length name prototype',
    'ReferenceError.prototype': 'constructor message name',
    'Reflect':
      'apply construct defineProperty deleteProperty get getOwnPropertyDescriptor ' +
      'getPrototypeOf has isExtensible ownKeys preventExtensions set setPrototypeOf',
    'RegExp': 'length name prototype',
    'RegExp.prototype':
      'constructor exec flags global ignoreCase lastIndex multiline source test toString',
    'String': 'fromCharCode fromCodePoint length name prototype',
    'String.prototype':
      'charAt charCodeAt codePointAt concat constructor endsWith includes indexOf lastIndexOf ' +
      'length localeCompare match repeat replace search slice split startsWith substr substring ' +
      'toLocaleLowerCase toLocaleUpperCase toLowerCase toString toUpperCase trim valueOf',
    'SyntaxError': 'length name prototype',
    'SyntaxError.prototype': 'constructor message name',
    'TextDecoder': 'length name prototype',
    'TextDecoder.prototype': 'constructor decode encoding fatal ignoreBOM',
    'TextEncoder': 'length name prototype',
    'TextEncoder.prototype': 'constructor encode encoding',
    'TypeError': 'length name prototype',
    'TypeError.prototype': 'constructor message name',
    'URIError': 'length name prototype',
    'URIError.prototype': 'constructor message name',
    'Uint16Array': 'BYTES_PER_ELEMENT length name prototype',
    'Uint16Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Uint32Array': 'BYTES_PER_ELEMENT length name prototype',
    'Uint32Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Uint8Array': 'BYTES_PER_ELEMENT allocPlain length name plainOf prototype',
    'Uint8Array.prototype': 'BYTES_PER_ELEMENT constructor',
    'Uint8ClampedArray': 'BYTES_PER_ELEMENT length name prototype',
    'Uint8ClampedArray.prototype': 'BYTES_PER_ELEMENT constructor',
    'decodeURI': 'length name',
    'decodeURIComponent': 'length name',
    'encodeURI': 'length name',
    'encodeURIComponent': 'length name',
    'escape': 'length name',
    'eval': 'length name',
    'isFinite': 'length name',
    'isNaN': 'length name',
    'parseFloat': 'length name',
    'parseInt': 'length name',
    'performance': 'now',
    'print': '',
    'require': 'name',
    'unescape': 'length name',
    '%TypedArray%': 'length name prototype',
    // Duktape gives every view an own `length`; QuickJS's views read the inherited getter.
    '%TypedArray%.prototype': 'buffer byteLength byteOffset set subarray length'
  };
  var typedArray = Object.getPrototypeOf(Uint8Array);
  var deleteProperty = Reflect.deleteProperty;

  // The object at `path`: 'global', a global's name, '%TypedArray%', or one of those plus
  // '.prototype'.
  function resolve(path) {
    var parts = path.split('.');
    var base = parts[0] === 'global' ? global
      : parts[0] === '%TypedArray%' ? typedArray : global[parts[0]];
    if (parts.length > 1 && base) {
      base = base[parts[1]];
    }
    return base;
  }

  // The objects are resolved before any global is deleted.
  var targets = Object.keys(members).map(function (path) {
    return { object: resolve(path), keep: members[path].split(' ') };
  });
  targets.forEach(function (target) {
    if (target.object === undefined || target.object === null) {
      return;
    }
    Object.getOwnPropertyNames(target.object).forEach(function (name) {
      if (target.keep.indexOf(name) < 0) {
        deleteProperty(target.object, name);
      }
    });
  });
})
