// Duktape 2.3 built-ins that QuickJS lacks or implements differently, reproduced on QuickJS so
// scripts see what they see under FMD2's Duktape (baseunits/Duktape.pas:77-104). Sources are
// Duktape 2.3.0's duktape.c (vendored in crates/fmd-duktape-ref) with its default config;
// tests/duktape_reference.rs compares each with a Duktape build. docs/duktape-differences.md
// lists what is not reproduced.
//
// Returns `native(fn)`, which marks a function written here in JS as one of Duktape's native
// functions for Function.prototype.toString.
(function () {
  'use strict';
  var define = Object.defineProperty;
  var fromCharCode = String.fromCharCode;
  var Bytes = Uint8Array;

  // The functions written here that are native functions in Duktape.
  var natives = new WeakSet();
  function native(fn) {
    natives.add(fn);
    return fn;
  }

  // Defines `value` on `target` like a built-in property: writable, configurable, not
  // enumerable. Functions are passed as methods (`{ name() {} }.name`), which like Duktape's
  // native functions have no `prototype`; this file only runs on QuickJS, so ES2015 syntax is
  // fine.
  function builtin(target, name, value) {
    if (typeof value === 'function') {
      native(value);
    }
    define(target, name, { value: value, writable: true, enumerable: false, configurable: true });
  }

  // Defines the accessors of `accessors` (`{ get name() {} }`) on `target` like built-in ones,
  // whose functions have no name of their own in Duktape.
  function getters(target, accessors) {
    Object.getOwnPropertyNames(accessors).forEach(function (name) {
      var get = native(Object.getOwnPropertyDescriptor(accessors, name).get);
      delete get.name;
      define(target, name, { get: get, enumerable: false, configurable: true });
    });
  }

  // A string's bytes in Duktape's internal encoding: each UTF-16 code unit is its own UTF-8
  // sequence, so a surrogate pair is two 3-byte sequences (CESU-8).
  function stringBytes(s) {
    var out = [];
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c < 0x80) {
        out.push(c);
      } else if (c < 0x800) {
        out.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
      } else {
        out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
    }
    return out;
  }

  // The bytes of a buffer (ArrayBuffer or a view on one), or null for anything else.
  function bufferBytes(value) {
    if (value instanceof ArrayBuffer) {
      return new Bytes(value);
    }
    if (ArrayBuffer.isView(value)) {
      return new Bytes(value.buffer, value.byteOffset, value.byteLength);
    }
    return null;
  }

  // duk__prep_codec_arg: a buffer's bytes, else those of ToString(value).
  function codecBytes(value) {
    return bufferBytes(value) || stringBytes(String(value));
  }

  // A string of the UTF-16 code units in `units`.
  function unitString(units) {
    var s = '';
    for (var i = 0; i < units.length; i += 8192) {
      s += fromCharCode.apply(null, units.slice(i, i + 8192));
    }
    return s;
  }

  var HEX = '0123456789abcdef';

  // duk_hex_encode: lower-case hex digits.
  function hexEncode(bytes) {
    var out = [];
    for (var i = 0; i < bytes.length; i++) {
      out.push(HEX.charAt(bytes[i] >> 4) + HEX.charAt(bytes[i] & 0xf));
    }
    return out.join('');
  }

  function hexDigit(b) {
    if (b >= 0x30 && b <= 0x39) return b - 0x30;
    if (b >= 0x61 && b <= 0x66) return b - 0x61 + 10;
    if (b >= 0x41 && b <= 0x46) return b - 0x41 + 10;
    return -1;
  }

  // duk_hex_decode: pairs of hex digits of either case; anything else fails.
  function hexDecode(bytes) {
    if (bytes.length % 2 !== 0) {
      throw new TypeError('hex decode failed');
    }
    var out = new Bytes(bytes.length / 2);
    for (var i = 0; i < out.length; i++) {
      var hi = hexDigit(bytes[2 * i]);
      var lo = hexDigit(bytes[2 * i + 1]);
      if (hi < 0 || lo < 0) {
        throw new TypeError('hex decode failed');
      }
      out[i] = (hi << 4) | lo;
    }
    return out;
  }

  var BASE64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

  // duk_base64_encode: standard alphabet, padded.
  function base64Encode(bytes) {
    var out = [];
    for (var i = 0; i < bytes.length; i += 3) {
      var n = bytes.length - i;
      var t = (bytes[i] << 16) | ((n > 1 ? bytes[i + 1] : 0) << 8) | (n > 2 ? bytes[i + 2] : 0);
      out.push(
        BASE64.charAt(t >> 18) + BASE64.charAt((t >> 12) & 0x3f) +
        (n > 1 ? BASE64.charAt((t >> 6) & 0x3f) : '=') + (n > 2 ? BASE64.charAt(t & 0x3f) : '='));
    }
    return out.join('');
  }

  // The value of base64 byte `b`: 0-63, -1 for skipped whitespace (tab, LF, CR, space), -2 for
  // padding, -3 for anything else (duk__base64_dectab_fast).
  function base64Value(b) {
    if (b === 0x3d) return -2;
    if (b === 0x09 || b === 0x0a || b === 0x0d || b === 0x20) return -1;
    var i = b < 0x80 ? BASE64.indexOf(fromCharCode(b)) : -1;
    return i < 0 ? -3 : i;
  }

  // duk_base64_decode (duk__base64_decode_helper): groups of up to four characters, whitespace
  // skipped anywhere. A group ends at four characters, padding or the end of input, so padding
  // may be missing, partial or extra, and padded documents may be concatenated. A group of
  // one character fails.
  function base64Decode(bytes) {
    var out = [];
    var i = 0;
    while (i < bytes.length) {
      var t = 0;
      var chars = 0;
      while (i < bytes.length && chars < 4) {
        var x = base64Value(bytes[i]);
        if (x === -2) {
          break;
        }
        if (x === -3) {
          throw new TypeError('base64 decode failed');
        }
        i++;
        if (x >= 0) {
          t = t * 64 + x;
          chars++;
        }
      }
      if (chars === 1) {
        throw new TypeError('base64 decode failed');
      }
      for (var k = chars; k < 4; k++) {
        t *= 64;
      }
      if (chars >= 2) out.push((t >> 16) & 0xff);
      if (chars >= 3) out.push((t >> 8) & 0xff);
      if (chars === 4) out.push(t & 0xff);
      while (i < bytes.length && (base64Value(bytes[i]) === -1 || base64Value(bytes[i]) === -2)) {
        i++;
      }
    }
    return new Bytes(out);
  }

  // duk_require_hstring and duk_require_valid_index on the arguments of Duktape.enc/dec.
  function checkCodecArgs(args) {
    if (typeof args[0] !== 'string') {
      throw new TypeError('string required');
    }
    if (args.length < 2) {
      throw new RangeError('invalid stack index 1');
    }
  }

  // The `Duktape` built-in (duk_bi_duktape.c). Duktape.modSearch and Duktape.modLoaded are added
  // by the module loader (prelude.js).
  var Duktape = {};
  builtin(Duktape, 'version', 20300);
  // What FMD2's build reports (dist/x86_64-win64/libduktape.dll).
  builtin(Duktape, 'env', 'll u nl p2 a8 x64 windows mingw');
  // duk_bi_duktape_object_enc: 'hex' and 'base64'. The 'jx' and 'jc' JSON formats are not
  // reproduced.
  builtin(Duktape, 'enc', { enc() {
    var format = arguments[0];
    var value = arguments[1];
    checkCodecArgs(arguments);
    if (format === 'hex') {
      return hexEncode(codecBytes(value));
    }
    if (format === 'base64') {
      return base64Encode(codecBytes(value));
    }
    throw new TypeError('invalid args');
  } }.enc);
  // duk_bi_duktape_object_dec: the decoded bytes, as a Uint8Array (Duktape's plain buffer).
  builtin(Duktape, 'dec', { dec() {
    var format = arguments[0];
    var value = arguments[1];
    checkCodecArgs(arguments);
    if (format === 'hex') {
      return hexDecode(codecBytes(value));
    }
    if (format === 'base64') {
      return base64Decode(codecBytes(value));
    }
    throw new TypeError('invalid args');
  } }.dec);
  builtin(Duktape, 'gc', { gc() {
    return true;
  } }.gc);
  builtin(Duktape, 'compact', { compact(obj) {
    return obj;
  } }.compact);
  builtin(globalThis, 'Duktape', Duktape);

  // A string of the code points in `codepoints`, those above U+FFFF as surrogate pairs.
  function codepointString(codepoints) {
    var units = [];
    for (var i = 0; i < codepoints.length; i++) {
      var c = codepoints[i];
      if (c > 0xffff) {
        c -= 0x10000;
        units.push(0xd800 | (c >> 10), 0xdc00 | (c & 0x3ff));
      } else {
        units.push(c);
      }
    }
    return unitString(units);
  }

  // TextDecoder (duk_bi_textdecoder_*): UTF-8 whatever the label, decoded as the Encoding
  // standard's UTF-8 decoder does (duk__utf8_decode_next), with `fatal`, `ignoreBOM` and
  // `stream`.
  var decoders = new WeakMap();

  function decoderState(decoder) {
    var state = decoders.get(decoder);
    if (state === undefined) {
      throw new TypeError('not TextDecoder');
    }
    return state;
  }

  function resetDecoder(state) {
    state.needed = 0;
    state.codepoint = 0;
    state.lower = 0x80;
    state.upper = 0xbf;
    state.bomHandled = false;
  }

  function TextDecoder() {
    var label = arguments[0];
    var options = arguments[1];
    if (!(this instanceof TextDecoder)) {
      throw new TypeError('constructor requires \'new\'');
    }
    if (label !== undefined) {
      String(label);
    }
    var state = { fatal: false, ignoreBOM: false };
    if (options !== undefined && options !== null) {
      if ('fatal' in Object(options)) state.fatal = !!options.fatal;
      if ('ignoreBOM' in Object(options)) state.ignoreBOM = !!options.ignoreBOM;
    }
    resetDecoder(state);
    decoders.set(this, state);
  }
  getters(TextDecoder.prototype, {
    get encoding() {
      decoderState(this);
      return 'utf-8';
    },
    get fatal() {
      return decoderState(this).fatal;
    },
    get ignoreBOM() {
      return decoderState(this).ignoreBOM;
    }
  });
  builtin(TextDecoder.prototype, 'decode', { decode() {
    var input = arguments[0];
    var options = arguments[1];
    var state = decoderState(this);
    var bytes = input === undefined ? new Bytes(0) : bufferBytes(input);
    if (bytes === null) {
      throw new TypeError('buffer required');
    }
    var stream = false;
    if (options !== undefined && options !== null) {
      if (typeof options !== 'object' && typeof options !== 'function') {
        throw new TypeError('unexpected type');
      }
      stream = !!options.stream;
    }
    var out = [];
    function emit(c) {
      if (!state.bomHandled) {
        state.bomHandled = true;
        if (c === 0xfeff && !state.ignoreBOM) {
          return;
        }
      }
      out.push(c);
    }
    function fail() {
      if (state.fatal) {
        resetDecoder(state);
        throw new TypeError('utf-8 decode failed');
      }
      emit(0xfffd);
    }
    for (var i = 0; i < bytes.length; i++) {
      var x = bytes[i];
      if (state.needed === 0) {
        if (x <= 0x7f) {
          emit(x);
        } else if (x >= 0xc2 && x <= 0xdf) {
          state.needed = 1;
          state.codepoint = x & 0x1f;
        } else if (x >= 0xe0 && x <= 0xef) {
          if (x === 0xe0) state.lower = 0xa0;
          if (x === 0xed) state.upper = 0x9f;
          state.needed = 2;
          state.codepoint = x & 0x0f;
        } else if (x >= 0xf0 && x <= 0xf4) {
          if (x === 0xf0) state.lower = 0x90;
          if (x === 0xf4) state.upper = 0x8f;
          state.needed = 3;
          state.codepoint = x & 0x07;
        } else {
          fail();
        }
      } else if (x >= state.lower && x <= state.upper) {
        state.lower = 0x80;
        state.upper = 0xbf;
        state.codepoint = (state.codepoint << 6) | (x & 0x3f);
        if (--state.needed === 0) {
          emit(state.codepoint);
          state.codepoint = 0;
        }
      } else {
        // An unexpected continuation byte ends the sequence and is decoded again.
        state.needed = 0;
        state.codepoint = 0;
        state.lower = 0x80;
        state.upper = 0xbf;
        i--;
        fail();
      }
    }
    if (!stream) {
      if (state.needed !== 0) {
        fail();
      }
      resetDecoder(state);
    }
    return codepointString(out);
  } }.decode);
  builtin(globalThis, 'TextDecoder', TextDecoder);

  // TextEncoder (duk_bi_textencoder_*): UTF-8, a lone surrogate as U+FFFD.
  function TextEncoder() {
    if (!(this instanceof TextEncoder)) {
      throw new TypeError('constructor requires \'new\'');
    }
  }
  getters(TextEncoder.prototype, {
    get encoding() {
      return 'utf-8';
    }
  });
  builtin(TextEncoder.prototype, 'encode', { encode() {
    var input = arguments[0];
    var s = input === undefined ? '' : String(input);
    var out = [];
    for (var i = 0; i < s.length; i++) {
      var c = s.charCodeAt(i);
      if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
        var d = s.charCodeAt(i + 1);
        if (d >= 0xdc00 && d <= 0xdfff) {
          c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
          i++;
        }
      }
      if (c >= 0xd800 && c <= 0xdfff) {
        c = 0xfffd;
      }
      if (c < 0x80) {
        out.push(c);
      } else if (c < 0x800) {
        out.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
      } else if (c < 0x10000) {
        out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      } else {
        out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 0x3f), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
    }
    return new Bytes(out);
  } }.encode);
  builtin(globalThis, 'TextEncoder', TextEncoder);

  // RegExp.prototype is itself a RegExp in Duktape (ES5), so it has its own `lastIndex`.
  define(RegExp.prototype, 'lastIndex', { value: 0, writable: true });

  // `performance` with only `now` (QuickJS's `timeOrigin` cannot be deleted).
  var quickPerformance = performance;
  var quickNow = performance.now;
  var duktapePerformance = {};
  builtin(duktapePerformance, 'now', { now() {
    return quickNow.call(quickPerformance);
  } }.now);
  globalThis.performance = duktapePerformance;

  // Duktape's plain-buffer helpers on Uint8Array (duk_bi_uint8array_allocplain/plainof); a
  // plain buffer is a Uint8Array here. allocPlain(n) is n zero bytes, or a copy of the bytes of a
  // buffer, a string (internal encoding) or an array; plainOf(view) is the whole buffer behind it.
  builtin(Bytes, 'allocPlain', { allocPlain(value) {
    if (typeof value === 'number') {
      return new Bytes(value);
    }
    if (typeof value === 'string') {
      return new Bytes(stringBytes(value));
    }
    var bytes = bufferBytes(value);
    return bytes !== null ? new Bytes(bytes) : new Bytes(value);
  } }.allocPlain);
  builtin(Bytes, 'plainOf', { plainOf(value) {
    if (value instanceof ArrayBuffer) {
      return new Bytes(value);
    }
    if (ArrayBuffer.isView(value)) {
      return new Bytes(value.buffer);
    }
    throw new TypeError('buffer required');
  } }.plainOf);

  // Dates as FMD2's Windows build of Duktape prints and parses them. That build has no platform
  // date formatter or parser (duk_config.h, "Windows"), so every string form is Duktape's ISO
  // 8601 one (duk__format_parts_iso8601), toLocale*String included, and strings are parsed by
  // its ISO 8601 subset parser alone (duk__parse_string_iso8601_subset).
  var QuickDate = Date;
  var quickGetTime = QuickDate.prototype.getTime;
  var construct = Reflect.construct;

  function pad(n, width) {
    var s = String(n);
    while (s.length < width) {
      s = '0' + s;
    }
    return s;
  }

  // The parts of the Date `date` in local time or UTC, or null when its time value is NaN.
  function dateParts(date, local) {
    var t = quickGetTime.call(date);
    if (t !== t) {
      return null;
    }
    var d = new QuickDate(t);
    return local
      ? [d.getFullYear(), d.getMonth() + 1, d.getDate(), d.getHours(), d.getMinutes(),
        d.getSeconds(), d.getMilliseconds(), -d.getTimezoneOffset()]
      : [d.getUTCFullYear(), d.getUTCMonth() + 1, d.getUTCDate(), d.getUTCHours(),
        d.getUTCMinutes(), d.getUTCSeconds(), d.getUTCMilliseconds()];
  }

  // duk__format_parts_iso8601: `YYYY-MM-DD`, `HH:MM:SS.mmm` and the offset (`+HH:MM` in local
  // time, `Z` in UTC), as `which` selects; years outside 0-9999 have a sign and six digits.
  function formatDate(date, local, which) {
    var p = dateParts(date, local);
    if (p === null) {
      return 'Invalid Date';
    }
    var year = p[0] >= 0 && p[0] <= 9999 ? pad(p[0], 4)
      : (p[0] >= 0 ? '+' : '-') + pad(Math.abs(p[0]), 6);
    var day = year + '-' + pad(p[1], 2) + '-' + pad(p[2], 2);
    var zone = 'Z';
    if (local) {
      var offset = Math.abs(p[7]);
      zone = (p[7] >= 0 ? '+' : '-') + pad(Math.floor(offset / 60), 2) + ':' + pad(offset % 60, 2);
    }
    var time = pad(p[3], 2) + ':' + pad(p[4], 2) + ':' + pad(p[5], 2) + '.' + pad(p[6], 3) + zone;
    return which === 'date' ? day : which === 'time' ? time : day + ' ' + time;
  }

  var dateMethods = {
    toString() { return formatDate(this, true, 'both'); },
    toDateString() { return formatDate(this, true, 'date'); },
    toTimeString() { return formatDate(this, true, 'time'); },
    toLocaleString() { return formatDate(this, true, 'both'); },
    toLocaleDateString() { return formatDate(this, true, 'date'); },
    toLocaleTimeString() { return formatDate(this, true, 'time'); },
    toUTCString() { return formatDate(this, false, 'both'); }
  };
  Object.keys(dateMethods).forEach(function (name) {
    builtin(QuickDate.prototype, name, dateMethods[name]);
  });
  builtin(QuickDate.prototype, 'toGMTString', dateMethods.toUTCString);

  // duk__parse_iso8601_control: which separator may follow which part, and what comes next.
  // A rule is a mask of parts (bits 0-8), a mask of separators (bits 9-16), the next part (bits
  // 17-20) and flags (bits 21+): 1 sets a negative offset, 2 accepts, 4 accepts at the end.
  var SEPARATORS = '+-T :.Z';
  function rule(parts, separators, next, flags) {
    var mask = 0;
    for (var i = 0; i < separators.length; i++) {
      mask |= 1 << (separators[i] === '\0' ? 7 : SEPARATORS.indexOf(separators[i]));
    }
    return parts + mask * 512 + next * 131072 + flags * 2097152;
  }
  var YMD = 1 | 2 | 4;
  var ANY_TIME = YMD | 8 | 16 | 32 | 64;
  var PARSE_RULES = [
    rule(1, '-', 1, 0),
    rule(2, '-', 2, 0),
    rule(YMD, 'T ', 3, 0),
    rule(8, ':', 4, 0),
    rule(16, ':', 5, 0),
    rule(32, '.', 6, 0),
    rule(128, ':', 8, 0),
    rule(ANY_TIME, '+', 7, 0),
    rule(ANY_TIME, '-', 7, 1),
    rule(ANY_TIME, 'Z', 0, 4),
    rule(ANY_TIME | 128 | 256, '\0', 0, 2)
  ];

  // duk__parse_string_iso8601_subset: year[-month[-day[(T| )hour[:minute[:second[.fraction]]]]]]
  // then Z, +hh[:mm] or -hh[:mm]; a missing offset is UTC. Any other string is NaN. The string
  // is read as a C string, so it ends at a NUL.
  function parseDate(str) {
    var nul = str.indexOf('\0');
    if (nul >= 0) {
      str = str.substring(0, nul);
    }
    // year, month, day, hour, minute, second, millisecond, offset hours, offset minutes
    var parts = [0, 1, 1, 0, 0, 0, 0, 0, 0];
    var part = 0;
    var accum = 0;
    var digits = 0;
    var negYear = false;
    var negOffset = false;
    var p = 0;
    if (str.charAt(0) === '+') {
      p++;
    } else if (str.charAt(0) === '-') {
      negYear = true;
      p++;
    }
    for (;;) {
      var c = p < str.length ? str.charCodeAt(p) : 0;
      p++;
      if (c >= 0x30 && c <= 0x39) {
        if (digits >= 9) {
          return NaN;
        }
        if (part !== 6 || digits < 3) {
          accum = accum * 10 + (c - 0x30);
          digits++;
        }
        continue;
      }
      if (digits <= 0) {
        return NaN;
      }
      if (part === 6) {
        while (digits < 3) {
          accum *= 10;
          digits++;
        }
      }
      parts[part] = accum;
      accum = 0;
      digits = 0;
      var separator = c === 0 ? 7 : c < 0x80 ? SEPARATORS.indexOf(String.fromCharCode(c)) : -1;
      if (separator < 0) {
        return NaN;
      }
      var match = (1 << part) + (1 << (separator + 9));
      var found = -1;
      for (var i = 0; i < PARSE_RULES.length; i++) {
        if ((PARSE_RULES[i] & match) === match) {
          found = PARSE_RULES[i];
          break;
        }
      }
      if (found < 0) {
        return NaN;
      }
      var flags = Math.floor(found / 2097152);
      if (flags & 1) {
        negOffset = true;
      }
      if (flags & 2) {
        break;
      }
      if (flags & 4) {
        if (p >= str.length) {
          break;
        }
        return NaN;
      }
      part = Math.floor(found / 131072) & 0xf;
      if (c === 0) {
        return NaN;
      }
    }
    var sign = negOffset ? 1 : -1;
    var year = negYear ? -parts[0] : parts[0];
    var hour = parts[3] + sign * parts[7];
    var minute = parts[4] + sign * parts[8];
    // Date.UTC reads years 0-99 as 1900-1999; 400 years later is the same calendar.
    if (year >= 0 && year <= 99) {
      return quickUTC(year + 400, parts[1] - 1, parts[2], hour, minute, parts[5], parts[6]) -
        146097 * 86400000;
    }
    return quickUTC(year, parts[1] - 1, parts[2], hour, minute, parts[5], parts[6]);
  }

  // ToPrimitive with no hint, as ES5's [[DefaultValue]]: a Date prefers its string, any other
  // object its number.
  function toPrimitive(value) {
    if (value === null || (typeof value !== 'object' && typeof value !== 'function')) {
      return value;
    }
    var order = value instanceof QuickDate ? ['toString', 'valueOf'] : ['valueOf', 'toString'];
    for (var i = 0; i < 2; i++) {
      var method = value[order[i]];
      if (typeof method === 'function') {
        var result = method.call(value);
        if (result === null || (typeof result !== 'object' && typeof result !== 'function')) {
          return result;
        }
      }
    }
    throw new TypeError('coercion to primitive failed');
  }

  // The Date constructor: `new Date(string)` parses with parseDate, `Date()` is the current time
  // as toString prints it; anything else is QuickJS's.
  var DuktapeDate = function Date(year, month, day, hours, minutes, seconds, ms) {
    if (new.target === undefined) {
      return formatDate(new QuickDate(), true, 'both');
    }
    if (arguments.length === 1) {
      var value = toPrimitive(year);
      return construct(QuickDate, [typeof value === 'string' ? parseDate(value) : value],
        new.target);
    }
    return construct(QuickDate, arguments, new.target);
  };
  define(DuktapeDate, 'prototype', { value: QuickDate.prototype, writable: false });
  builtin(QuickDate.prototype, 'constructor', DuktapeDate);
  // ES5's Date.UTC: a missing month is NaN, so is the result (ES2017 defaults it to 0).
  var quickUTC = QuickDate.UTC;
  builtin(DuktapeDate, 'UTC', { UTC(year, month, date, hours, minutes, seconds, ms) {
    if (arguments.length < 2) {
      Number(year);
      return NaN;
    }
    return quickUTC.apply(null, arguments);
  } }.UTC);
  builtin(DuktapeDate, 'now', QuickDate.now);
  builtin(DuktapeDate, 'parse', { parse(string) {
    return parseDate(String(string));
  } }.parse);
  builtin(globalThis, 'Date', DuktapeDate);

  // Function.prototype.toString (duk_bi_function_prototype_to_string): `function NAME() {
  // [ecmascript code] }`, with `[native code]` for native functions and `[bound code]` for
  // bound ones, NAME being ToString(this.name) or '' when that is undefined. Bound functions are
  // the ones `bind` returned; native ones are QuickJS's (which it prints as below) and the
  // functions marked with `native`.
  var quickToString = Function.prototype.toString;
  var quickBind = Function.prototype.bind;
  var QUICKJS_NATIVE = /^function [^(]*\(\) \{\n {4}\[native code\]\n\}$/;
  var bound = new WeakSet();
  builtin(Function.prototype, 'bind', { bind(thisArg) {
    var fn = quickBind.apply(this, arguments);
    bound.add(fn);
    return fn;
  } }.bind);
  builtin(Function.prototype, 'toString', { toString() {
    var source = quickToString.call(this);
    var name = this.name;
    name = name === undefined ? '' : String(name);
    var kind = bound.has(this) ? 'bound'
      : natives.has(this) || QUICKJS_NATIVE.test(source) ? 'native' : 'ecmascript';
    return 'function ' + name + '() { [' + kind + ' code] }';
  } }.toString);

  // Duktape's JSON.stringify writes lone surrogates as they are and escapes U+2028 and U+2029
  // (DUK_USE_NONSTD_JSON_ESC_U2028_U2029); QuickJS does the opposite (ES2019). An escaped
  // backslash is matched first so `\\ud800` stays as it is.
  var quickStringify = JSON.stringify;
  builtin(JSON, 'stringify', { stringify(value, replacer, space) {
    var json = quickStringify.apply(JSON, arguments);
    if (typeof json !== 'string') {
      return json;
    }
    return json.replace(/\\\\|\\u(d[89a-f][0-9a-f]{2})|[\u2028\u2029]/gi, function (m, surrogate) {
      if (surrogate !== undefined) {
        return fromCharCode(parseInt(surrogate, 16));
      }
      return m.length === 1 ? '\\u' + m.charCodeAt(0).toString(16) : m;
    });
  } }.stringify);

  return native;
})
