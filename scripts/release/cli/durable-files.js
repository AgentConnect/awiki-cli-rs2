'use strict';
const fs = require('node:fs');
const path = require('node:path');

function sync(file) {
  const fd = fs.openSync(file, 'r');
  try { fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
}
function mkdir(directory) {
  const missing = [];
  for (let p = path.resolve(directory); !fs.existsSync(p); p = path.dirname(p)) missing.push(p);
  fs.mkdirSync(directory, { recursive: true });
  for (const p of missing) { sync(p); sync(path.dirname(p)); }
}
function write(file, content, mode = 0o600) {
  mkdir(path.dirname(file));
  const tmp = `${file}.tmp-${process.pid}`;
  const fd = fs.openSync(tmp, 'w', mode);
  try { fs.writeFileSync(fd, content); fs.fchmodSync(fd, mode); fs.fsyncSync(fd); }
  finally { fs.closeSync(fd); }
  fs.renameSync(tmp, file);
  sync(path.dirname(file));
}
function json(file, value) { write(file, JSON.stringify(value, null, 2) + '\n'); }
function link(target, pointer) {
  mkdir(path.dirname(pointer));
  try {
    if (!fs.lstatSync(pointer).isSymbolicLink()) throw new Error(`pointer must be symlink: ${pointer}`);
  } catch (e) { if (e.code !== 'ENOENT') throw e; }
  const tmp = `${pointer}.tmp-${process.pid}`;
  fs.rmSync(tmp, { force: true });
  fs.symlinkSync(target, tmp);
  fs.renameSync(tmp, pointer);
  sync(path.dirname(pointer));
}
function restore(pointer, old) {
  if (old !== null) link(old, pointer);
  else {
    try { fs.unlinkSync(pointer); } catch (e) { if (e.code !== 'ENOENT') throw e; }
    sync(path.dirname(pointer));
  }
}
function tree(directory) {
  for (const name of fs.readdirSync(directory)) {
    const file = path.join(directory, name), stat = fs.lstatSync(file);
    if (stat.isDirectory()) tree(file);
    else if (stat.isFile()) sync(file);
    else throw new Error(`release contains non-regular file: ${file}`);
  }
  sync(directory);
}
module.exports = { mkdir, sync, write, json, link, restore, tree };
