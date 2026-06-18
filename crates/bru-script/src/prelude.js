// Safe-Mode prelude. The host injects __varsJson / __reqJson / __resJson as
// strings; this builds the bru / req / res / test / expect / pm API on top, and
// exposes __vars / __tests / __console for the host to read back. No filesystem,
// network, process, or require() exists in this sandbox.

globalThis.__vars = JSON.parse(__varsJson);
globalThis.__req = JSON.parse(__reqJson);
globalThis.__res = JSON.parse(__resJson);
globalThis.__tests = [];
globalThis.__console = [];

function __fmt(v) {
  if (typeof v === 'string') return v;
  try { return JSON.stringify(v); } catch (e) { return String(v); }
}

globalThis.console = {
  log: function () { __console.push(Array.prototype.map.call(arguments, __fmt).join(' ')); },
  info: function () { globalThis.console.log.apply(null, arguments); },
  warn: function () { globalThis.console.log.apply(null, arguments); },
  error: function () { globalThis.console.log.apply(null, arguments); },
  debug: function () { globalThis.console.log.apply(null, arguments); },
};

globalThis.bru = {
  getVar: function (k) { return __vars[k]; },
  setVar: function (k, v) { __vars[k] = (v === undefined || v === null) ? '' : String(v); },
  hasVar: function (k) { return Object.prototype.hasOwnProperty.call(__vars, k); },
  deleteVar: function (k) { delete __vars[k]; },
  getEnvVar: function (k) { return __vars[k]; },
  setEnvVar: function (k, v) { bru.setVar(k, v); },
  getProcessEnv: function (k) { return __vars[k]; },
  isSafeMode: function () { return true; },
};

function __headerGet(headers, name) {
  if (!headers) return undefined;
  name = String(name).toLowerCase();
  for (var i = 0; i < headers.length; i++) {
    if (String(headers[i][0]).toLowerCase() === name) return headers[i][1];
  }
  return undefined;
}

globalThis.req = {
  url: __req.url,
  method: __req.method,
  getUrl: function () { return __req.url; },
  getMethod: function () { return __req.method; },
  getHeader: function (n) { return __headerGet(__req.headers, n); },
  getHeaders: function () { return __req.headers; },
};

globalThis.res = __res ? {
  status: __res.status,
  statusText: __res.statusText,
  body: __res.body,
  responseTime: __res.responseTime,
  getStatus: function () { return __res.status; },
  getBody: function () { return __res.body; },
  getHeader: function (n) { return __headerGet(__res.headers, n); },
  getHeaders: function () { return __res.headers; },
  getResponseTime: function () { return __res.responseTime; },
} : undefined;

globalThis.test = function (name, fn) {
  try {
    fn();
    __tests.push({ name: String(name), passed: true });
  } catch (e) {
    __tests.push({ name: String(name), passed: false, error: (e && e.message) ? String(e.message) : String(e) });
  }
};

// Minimal chai-style expect (throwing assertions; supports `.not`).
function __eq(a, b) { return JSON.stringify(a) === JSON.stringify(b); }
function __check(cond, msg, neg) { if (neg) cond = !cond; if (!cond) throw new Error(msg); }

function Assertion(actual, negated) { this.actual = actual; this.negated = !!negated; }
Assertion.prototype = {
  get to() { return this; },
  get be() { return this; },
  get been() { return this; },
  get is() { return this; },
  get that() { return this; },
  get which() { return this; },
  get has() { return this; },
  get have() { return this; },
  get with() { return this; },
  get and() { return this; },
  get a() { return this; },
  get an() { return this; },
  get not() { return new Assertion(this.actual, !this.negated); },
  equal: function (v) { __check(this.actual === v, 'expected ' + __fmt(this.actual) + ' to equal ' + __fmt(v), this.negated); return this; },
  equals: function (v) { return this.equal(v); },
  eql: function (v) { __check(__eq(this.actual, v), 'expected ' + __fmt(this.actual) + ' to deeply equal ' + __fmt(v), this.negated); return this; },
  above: function (v) { __check(this.actual > v, 'expected ' + __fmt(this.actual) + ' to be above ' + __fmt(v), this.negated); return this; },
  gt: function (v) { return this.above(v); },
  below: function (v) { __check(this.actual < v, 'expected ' + __fmt(this.actual) + ' to be below ' + __fmt(v), this.negated); return this; },
  lt: function (v) { return this.below(v); },
  least: function (v) { __check(this.actual >= v, 'expected ' + __fmt(this.actual) + ' to be at least ' + __fmt(v), this.negated); return this; },
  most: function (v) { __check(this.actual <= v, 'expected ' + __fmt(this.actual) + ' to be at most ' + __fmt(v), this.negated); return this; },
  include: function (v) {
    var ok = (typeof this.actual === 'string') ? this.actual.indexOf(v) >= 0
      : (Array.isArray(this.actual) ? this.actual.indexOf(v) >= 0 : false);
    __check(ok, 'expected ' + __fmt(this.actual) + ' to include ' + __fmt(v), this.negated); return this;
  },
  contain: function (v) { return this.include(v); },
  match: function (re) { __check(re.test(this.actual), 'expected ' + __fmt(this.actual) + ' to match ' + re, this.negated); return this; },
  property: function (name) { __check(this.actual != null && Object.prototype.hasOwnProperty.call(this.actual, name), 'expected object to have property ' + name, this.negated); return this; },
  lengthOf: function (n) { __check(this.actual != null && this.actual.length === n, 'expected length ' + n, this.negated); return this; },
  oneOf: function (arr) { __check(arr.indexOf(this.actual) >= 0, 'expected ' + __fmt(this.actual) + ' to be one of ' + __fmt(arr), this.negated); return this; },
  get true() { __check(this.actual === true, 'expected ' + __fmt(this.actual) + ' to be true', this.negated); return this; },
  get false() { __check(this.actual === false, 'expected ' + __fmt(this.actual) + ' to be false', this.negated); return this; },
  get null() { __check(this.actual === null, 'expected ' + __fmt(this.actual) + ' to be null', this.negated); return this; },
  get undefined() { __check(this.actual === undefined, 'expected ' + __fmt(this.actual) + ' to be undefined', this.negated); return this; },
  get ok() { __check(!!this.actual, 'expected ' + __fmt(this.actual) + ' to be truthy', this.negated); return this; },
  get empty() {
    var a = this.actual;
    var e = (a == null) || (a.length === 0) || (typeof a === 'object' && Object.keys(a).length === 0);
    __check(e, 'expected ' + __fmt(a) + ' to be empty', this.negated); return this;
  },
};
globalThis.expect = function (actual) { return new Assertion(actual, false); };

// Minimal Postman (pm.*) shim so translated/imported scripts mostly run.
globalThis.pm = {
  test: globalThis.test,
  expect: globalThis.expect,
  environment: { get: bru.getVar, set: bru.setVar, has: bru.hasVar, unset: bru.deleteVar },
  variables: { get: bru.getVar, set: bru.setVar, has: bru.hasVar },
  globals: { get: bru.getVar, set: bru.setVar },
  collectionVariables: { get: bru.getVar, set: bru.setVar },
  response: __res ? {
    code: __res.status,
    status: __res.statusText,
    responseTime: __res.responseTime,
    json: function () { return __res.body; },
    text: function () { return (typeof __res.body === 'string') ? __res.body : JSON.stringify(__res.body); },
    to: {
      have: {
        status: function (c) {
          if (typeof c === 'number') { if (__res.status !== c) throw new Error('expected status ' + c + ' but got ' + __res.status); }
          else { if (__res.statusText !== c) throw new Error('expected status ' + c + ' but got ' + __res.statusText); }
        },
      },
    },
  } : undefined,
};
globalThis.postman = globalThis.pm;
