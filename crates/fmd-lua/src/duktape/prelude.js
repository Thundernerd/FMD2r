// The globals FMD2 adds to every Duktape heap (baseunits/Duktape.pas:88-90):
// - `print(...)`, which logs its arguments joined by spaces (baseunits/Duktape.pas:22-30), through
//   the host's `log(text)`;
// - CommonJS `require` as Duktape 2.3's module loader implements it (duk_module_duktape_init,
//   baseunits/Duktape.Api.pas:1408; extras/module-duktape in the Duktape sources), with
//   FMD2's `Duktape.modSearch` (baseunits/Duktape.pas:39-75) supplied by the host as
//   `modSearch(id)`: the module's source, or undefined when no file exists. Not reproduced: the
//   global `Duktape` object (`Duktape.modLoaded`, `Duktape.modSearch`) and `module.filename` /
//   `module.name`, which none of the JS files under lua/ use.
(function (modSearch, log) {
  'use strict';
  var geval = eval;
  var hasOwn = Object.prototype.hasOwnProperty;
  // Duktape.modLoaded: one module object per resolved id, registered before the module runs so
  // cyclic requires see its partial exports.
  var modLoaded = {};

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

  function makeRequire(moduleId) {
    var require = function (id) {
      var resolved = resolve(String(id), moduleId);
      if (hasOwn.call(modLoaded, resolved)) {
        return modLoaded[resolved].exports;
      }
      var exports = {};
      var module = { exports: exports, id: resolved };
      var moduleRequire = makeRequire(resolved);
      modLoaded[resolved] = module;
      try {
        var source = modSearch(resolved);
        if (typeof source === 'string') {
          var body = geval('(function (require, exports, module) {' + source + '\n})');
          body.call(exports, moduleRequire, exports, module);
        }
      } catch (e) {
        delete modLoaded[resolved];
        throw e;
      }
      return module.exports;
    };
    if (moduleId !== undefined) {
      require.id = moduleId;
    }
    return require;
  }

  globalThis.print = function () {
    log(Array.prototype.map.call(arguments, String).join(' '));
  };
  globalThis.require = makeRequire(undefined);
})
