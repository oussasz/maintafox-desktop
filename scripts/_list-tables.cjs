const path = require("path");
const Database = require("better-sqlite3");
const db = new Database(path.join(process.env.APPDATA, "systems.maintafox.desktop", "maintafox.db"), { readonly: true });
const tables = db.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").all();
tables.forEach(t => console.log(t.name));
db.close();
