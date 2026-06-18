// CommonJS require() for Developer Mode only. Safe Mode never installs this —
// there is no require() there. The host fn __bru_load_module(fromDir, spec) does
// path resolution + file reads and returns {path, dir, source, json}; this builds
// the module wrapper, caches by resolved path, and binds each module its own
// require() rooted at its own directory (so nested relative requires resolve
// correctly).
(function () {
  var cache = Object.create(null);
  function makeRequire(baseDir) {
    return function require(spec) {
      var info = JSON.parse(__bru_load_module(baseDir, String(spec)));
      if (cache[info.path]) return cache[info.path].exports;
      var module = { exports: {} };
      cache[info.path] = module; // cache before exec so cycles resolve to the partial export
      if (info.json) {
        module.exports = JSON.parse(info.source);
      } else {
        var fn = new Function(
          'module',
          'exports',
          'require',
          '__dirname',
          '__filename',
          info.source
        );
        fn(module, module.exports, makeRequire(info.dir), info.dir, info.path);
      }
      return module.exports;
    };
  }
  globalThis.require = makeRequire(typeof __scriptDir === 'string' ? __scriptDir : '');
})();
