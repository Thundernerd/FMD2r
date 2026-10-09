// The globals FMD2 adds to every Duktape heap (baseunits/Duktape.pas:88-90):
// - `print(...)`, which logs its arguments joined by spaces (baseunits/Duktape.pas:22-30), through
//   the host's `log(text)`;
// - CommonJS `require` as Duktape 2.3's module loader implements it (duk_module_duktape_init,
//   baseunits/Duktape.Api.pas:1408; extras/module-duktape/duk_module_duktape.c in the Duktape
//   sources), with `Duktape.modLoaded` and FMD2's `Duktape.modSearch`
//   (baseunits/Duktape.pas:39-75), which calls the host's `modSearch(id)`: the module's source,
//   or undefined when no file exists.
(function (modSearch, log, native) {
  'use strict';
  var geval = eval;
  var hasOwn = Object.prototype.hasOwnProperty;
  var define = Object.defineProperty;
  // The loader reads Duktape.modLoaded and Duktape.modSearch from the `Duktape` object it was
  // set up with, whatever the global holds later.
  var Duktape = globalThis.Duktape;
  // Duktape.modLoaded: one module object per resolved id, registered before the module runs so
  // cyclic requires see its partial exports.
  define(Duktape, 'modLoaded', { value: Object.create(null), writable: true, configurable: true });
  // FMD2's modSearch (baseunits/Duktape.pas:39-67) is called with (id, require, exports, module)
  // and reads only the id.
  define(Duktape, 'modSearch', {
    value: native({ modSearch(id) {
      return modSearch(String(id));
    } }.modSearch),
    writable: true,
    configurable: true
  });

  // Resolves `requested` against the id of the module calling `require` (undefined at the top
  // level) term by term: "." is dropped, ".." pops a term, repeated slashes collapse, and an id
  // that is empty, absolute, ends in "/" or climbs above the root is an error.
  function resolve(requested, parentId) {
    var path = parentId !== undefined && requested.charAt(0) === '.'
      ? parentId + '/../' + requested
      : requested;
    var terms = [];
    var i = 0;
    for (;;) {
      if (i >= path.length || path.charAt(i) === '/') {
        throw new TypeError('cannot resolve module id: ' + requested);
      }
      if (path.substr(i, 2) === './') {
        i += 2;
      } else if (path.substr(i, 3) === '../') {
        if (terms.length === 0) {
          throw new TypeError('cannot resolve module id: ' + requested);
        }
        terms.pop();
        i += 3;
      } else {
        var end = path.indexOf('/', i);
        if (end < 0) {
          terms.push(path.substring(i));
          break;
        }
        terms.push(path.substring(i, end));
        i = end + 1;
      }
      while (path.charAt(i) === '/') {
        i++;
      }
    }
    var resolved = terms.join('/');
    if (resolved.length > 255) {
      throw new TypeError('cannot resolve module id: ' + requested);
    }
    return resolved;
  }

  // The `require` of the module `moduleId` (undefined at the top level), as duk__require:
  // a module found in Duktape.modLoaded returns its exports; otherwise a module object is
  // registered, Duktape.modSearch gives its source, and the source runs wrapped in a function
  // named after the id's last term (or `module.name`) with `this` the module's first exports.
  function makeRequire(moduleId) {
    // A method, so it has no `prototype`, like the native function Duktape creates.
    var require = native({ require(id) {
      if (typeof id !== 'string') {
        throw new TypeError('string required');
      }
      var resolved = resolve(id, moduleId);
      var modLoaded = Duktape.modLoaded;
      if (modLoaded === null ||
          (typeof modLoaded !== 'object' && typeof modLoaded !== 'function')) {
        throw new TypeError('object required');
      }
      var cached = modLoaded[resolved];
      if (cached !== undefined) {
        return cached.exports;
      }
      var exports = {};
      var module = {};
      define(module, 'exports', { value: exports, writable: true, configurable: true });
      define(module, 'id', { value: resolved });
      modLoaded[resolved] = module;
      var moduleRequire = makeRequire(resolved);
      try {
        var source = Duktape.modSearch(resolved, moduleRequire, exports, module);
        if (typeof source === 'string') {
          var body = geval('(function(require,exports,module){' + source + '\n})');
          var name = module.name;
          define(body, 'name', {
            value: name !== undefined ? name : resolved.substring(resolved.lastIndexOf('/') + 1)
          });
          body.call(exports, moduleRequire, module.exports, module);
        }
      } catch (e) {
        delete modLoaded[resolved];
        throw e;
      }
      return module.exports;
    } }.require);
    define(require, 'name', { value: 'require' });
    if (moduleId !== undefined) {
      define(require, 'id', { value: moduleId, configurable: true });
    }
    return require;
  }

  globalThis.print = native({ print() {
    log(Array.prototype.map.call(arguments, String).join(' '));
  } }.print);
  define(globalThis, 'require', {
    value: makeRequire(undefined),
    writable: true,
    configurable: true
  });
})
