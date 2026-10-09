// Duktape 2.3 built-ins that QuickJS lacks or implements differently, reproduced on QuickJS so
// scripts see what they see under FMD2's Duktape (baseunits/Duktape.pas:77-104). Sources are
// Duktape 2.3.0's duktape.c (vendored in crates/fmd-duktape-ref) with its default config;
// tests/duktape_reference.rs compares each with a Duktape build. docs/duktape-differences.md
// lists what is not reproduced.
(function () {
  'use strict';
  var define = Object.defineProperty;
  var fromCharCode = String.fromCharCode;
  var Bytes = Uint8Array;

  // Defines `value` on `target` like a built-in property: writable, configurable, not
  // enumerable.
  function builtin(target, name, value) {
    define(target, name, { value: value, writable: true, enumerable: false, configurable: true });
  }

  function getter(target, name, get) {
    define(target, name, { get: get, enumerable: false, configurable: true });
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
  builtin(Duktape, 'env', 'll u n p2 a8 x64');
  // duk_bi_duktape_object_enc: 'hex' and 'base64'. The 'jx' and 'jc' JSON formats are not
  // reproduced.
  builtin(Duktape, 'enc', function enc(format, value) {
    checkCodecArgs(arguments);
    if (format === 'hex') {
      return hexEncode(codecBytes(value));
    }
    if (format === 'base64') {
      return base64Encode(codecBytes(value));
    }
    throw new TypeError('invalid args');
  });
  // duk_bi_duktape_object_dec: the decoded bytes, as a Uint8Array (Duktape's plain buffer).
  builtin(Duktape, 'dec', function dec(format, value) {
    checkCodecArgs(arguments);
    if (format === 'hex') {
      return hexDecode(codecBytes(value));
    }
    if (format === 'base64') {
      return base64Decode(codecBytes(value));
    }
    throw new TypeError('invalid args');
  });
  builtin(Duktape, 'gc', function gc() {
    return true;
  });
  builtin(Duktape, 'compact', function compact(obj) {
    return obj;
  });
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

  function TextDecoder(label, options) {
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
  getter(TextDecoder.prototype, 'encoding', function () {
    decoderState(this);
    return 'utf-8';
  });
  getter(TextDecoder.prototype, 'fatal', function () {
    return decoderState(this).fatal;
  });
  getter(TextDecoder.prototype, 'ignoreBOM', function () {
    return decoderState(this).ignoreBOM;
  });
  builtin(TextDecoder.prototype, 'decode', function decode(input, options) {
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
  });
  builtin(globalThis, 'TextDecoder', TextDecoder);

  // TextEncoder (duk_bi_textencoder_*): UTF-8, a lone surrogate as U+FFFD.
  function TextEncoder() {
    if (!(this instanceof TextEncoder)) {
      throw new TypeError('constructor requires \'new\'');
    }
  }
  getter(TextEncoder.prototype, 'encoding', function () {
    return 'utf-8';
  });
  builtin(TextEncoder.prototype, 'encode', function encode(input) {
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
  });
  builtin(globalThis, 'TextEncoder', TextEncoder);

  // Duktape's JSON.stringify writes lone surrogates as they are and escapes U+2028 and U+2029
  // (DUK_USE_NONSTD_JSON_ESC_U2028_U2029); QuickJS does the opposite (ES2019). An escaped
  // backslash is matched first so `\\ud800` stays as it is.
  var quickStringify = JSON.stringify;
  builtin(JSON, 'stringify', function stringify(value, replacer, space) {
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
  });
})
