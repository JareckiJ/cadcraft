//! IndexedDB persistence for imported browser drawings.

use wasm_bindgen::prelude::*;
use web_sys::js_sys;

#[wasm_bindgen(inline_js = r#"
const DB_NAME = "cadcraft-recent-v1";
const STORE_NAME = "drawings";

function openDatabase() {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(DB_NAME, 1);

        request.onupgradeneeded = () => {
            const db = request.result;
            if (!db.objectStoreNames.contains(STORE_NAME)) {
                db.createObjectStore(STORE_NAME);
            }
        };

        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
}

export async function cadcraftRecentPut(name, bytes) {
    const db = await openDatabase();
    const id = crypto.randomUUID();
    const key = "cadcraft-cache:" + id + "/" + name;

    try {
        await new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, "readwrite");
            const store = tx.objectStore(STORE_NAME);

            store.put({
                name,
                bytes: new Uint8Array(bytes)
            }, key);

            tx.oncomplete = () => resolve();
            tx.onerror = () => reject(tx.error);
            tx.onabort = () => reject(tx.error);
        });

        return key;
    } finally {
        db.close();
    }
}

export async function cadcraftRecentGet(key) {
    const db = await openDatabase();

    try {
        return await new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, "readonly");
            const request = tx.objectStore(STORE_NAME).get(key);

            request.onsuccess = () => {
                if (!request.result) {
                    reject(new Error("Cached file is missing: " + key));
                } else {
                    resolve([
                        request.result.name,
                        request.result.bytes
                    ]);
                }
            };

            request.onerror = () => reject(request.error);
        });
    } finally {
        db.close();
    }
}

// CADCRAFT_RECENT_CACHE_CLEANUP
export async function cadcraftRecentPrune(keepKeys) {
    const db = await openDatabase();

    try {
        await new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, "readwrite");
            const store = tx.objectStore(STORE_NAME);
            const keep = new Set(keepKeys);
            const request = store.openCursor();

            request.onsuccess = () => {
                const cursor = request.result;
                if (!cursor) return;

                const key = cursor.key;
                if (
                    typeof key === "string" &&
                    key.startsWith("cadcraft-cache:") &&
                    !keep.has(key)
                ) {
                    cursor.delete();
                }
                cursor.continue();
            };

            tx.oncomplete = () => resolve();
            tx.onerror = () => reject(tx.error);
            tx.onabort = () => reject(tx.error);
        });
    } finally {
        db.close();
    }
}
"#)]
extern "C" {
    #[wasm_bindgen(catch, js_name = cadcraftRecentPut)]
    async fn recent_put_js(
        name: &str,
        bytes: &js_sys::Uint8Array,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = cadcraftRecentGet)]
    async fn recent_get_js(key: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_name = cadcraftRecentPrune)]
    async fn recent_prune_js(
        keep: &js_sys::Array,
    ) -> Result<JsValue, JsValue>;
}

fn error_string(error: JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("{error:?}"))
}

pub async fn put(name: &str, bytes: &[u8]) -> Result<String, String> {
    let data = js_sys::Uint8Array::from(bytes);

    let result = recent_put_js(name, &data)
        .await
        .map_err(error_string)?;

    result
        .as_string()
        .ok_or_else(|| "IndexedDB returned an invalid cache key".to_string())
}

pub async fn get(key: &str) -> Result<(String, Vec<u8>), String> {
    let result = recent_get_js(key).await.map_err(error_string)?;
    let items = js_sys::Array::from(&result);

    let name = items.get(0)
        .as_string()
        .ok_or("Cached filename is invalid")?;

    let data = js_sys::Uint8Array::new(&items.get(1)).to_vec();

    Ok((name, data))
}

/// Delete browser-cached drawings not present in the current history.
pub async fn prune(keep: &[String]) -> Result<(), String> {
    let keys = js_sys::Array::new();

    for key in keep {
        if key.starts_with("cadcraft-cache:") {
            keys.push(&JsValue::from_str(key));
        }
    }

    recent_prune_js(&keys).await.map_err(error_string)?;
    Ok(())
}
