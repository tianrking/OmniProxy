use anyhow::{Context, Result};
use hudsucker::{Body, hyper::Request, hyper::Response};
use serde::Serialize;
use std::{fs, path::Path};
use tokio::time::{Duration, timeout};
use tracing::{debug, info, warn};
use wasmtime::{Engine, Instance, Memory, Module, Store, TypedFunc};

#[derive(Clone)]
pub struct WasmPluginHost {
    engine: Engine,
    plugins: Vec<WasmPlugin>,
    timeout_ms: u64,
}

#[derive(Clone)]
struct WasmPlugin {
    name: String,
    module: Module,
}

#[derive(Serialize)]
struct RequestSnapshot {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
}

#[derive(Serialize)]
struct ResponseSnapshot {
    status: u16,
    headers: Vec<(String, String)>,
}

impl WasmPluginHost {
    pub fn load(plugin_dir: &Path, timeout_ms: u64) -> Result<Self> {
        if !plugin_dir.exists() {
            std::fs::create_dir_all(plugin_dir)
                .with_context(|| format!("create plugin dir {}", plugin_dir.display()))?;
        }

        let engine = Engine::default();
        let mut plugins = Vec::new();

        for entry in fs::read_dir(plugin_dir)
            .with_context(|| format!("read plugin dir {}", plugin_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("wasm") {
                continue;
            }

            match Module::from_file(&engine, &path) {
                Ok(module) => {
                    let name = path
                        .file_stem()
                        .and_then(|x| x.to_str())
                        .unwrap_or("unnamed")
                        .to_string();
                    info!(plugin = %name, path = %path.display(), "loaded wasm plugin");
                    plugins.push(WasmPlugin { name, module });
                }
                Err(err) => {
                    warn!(path = %path.display(), error = %err, "skip invalid wasm plugin");
                }
            }
        }

        Ok(Self {
            engine,
            plugins,
            timeout_ms,
        })
    }

    pub async fn eval_request_isolated(&self, req: &Request<Body>) -> Result<()> {
        let this = self.clone();
        let snapshot = req_for_blocking(req);
        let task =
            tokio::task::spawn_blocking(move || this.eval_request_blocking_snapshot(snapshot));
        match timeout(Duration::from_millis(self.timeout_ms), task).await {
            Ok(join_res) => join_res?,
            Err(_) => anyhow::bail!("wasm request plugin timeout: {}ms", self.timeout_ms),
        }
    }

    pub async fn eval_response_isolated(&self, res: &Response<Body>) -> Result<()> {
        let this = self.clone();
        let snapshot = res_for_blocking(res);
        let task =
            tokio::task::spawn_blocking(move || this.eval_response_blocking_snapshot(snapshot));
        match timeout(Duration::from_millis(self.timeout_ms), task).await {
            Ok(join_res) => join_res?,
            Err(_) => anyhow::bail!("wasm response plugin timeout: {}ms", self.timeout_ms),
        }
    }

    fn eval_request_blocking_snapshot(&self, snapshot: RequestSnapshot) -> Result<()> {
        let payload = serde_json::to_vec(&snapshot)?;
        self.broadcast("on_http_request", &payload)?;
        Ok(())
    }

    fn eval_response_blocking_snapshot(&self, snapshot: ResponseSnapshot) -> Result<()> {
        let payload = serde_json::to_vec(&snapshot)?;
        self.broadcast("on_http_response", &payload)?;
        Ok(())
    }

    fn broadcast(&self, hook: &str, payload: &[u8]) -> Result<()> {
        for plugin in &self.plugins {
            self.invoke_hook(plugin, hook, payload)
                .with_context(|| format!("plugin {}", plugin.name))?;
        }
        Ok(())
    }

    fn invoke_hook(&self, plugin: &WasmPlugin, hook: &str, payload: &[u8]) -> Result<()> {
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &plugin.module, &[])
            .with_context(|| format!("instantiate {}", plugin.name))?;

        let Some(hook_fn) = get_hook::<(i32, i32), i32>(&mut store, &instance, hook)? else {
            debug!(plugin = %plugin.name, hook, "hook not found, skip");
            return Ok(());
        };

        let Some(alloc_fn) = get_hook::<i32, i32>(&mut store, &instance, "alloc")? else {
            debug!(plugin = %plugin.name, "alloc not found, skip hook");
            return Ok(());
        };

        let Some(dealloc_fn) = get_hook::<(i32, i32), ()>(&mut store, &instance, "dealloc")? else {
            debug!(plugin = %plugin.name, "dealloc not found, skip hook");
            return Ok(());
        };

        let memory = instance
            .get_memory(&mut store, "memory")
            .context("memory export not found")?;

        let ptr = alloc_fn.call(&mut store, payload.len() as i32)?;
        write_memory(&mut store, memory, ptr as usize, payload)?;

        let rc = hook_fn.call(&mut store, (ptr, payload.len() as i32))?;
        dealloc_fn.call(&mut store, (ptr, payload.len() as i32))?;

        if rc != 0 {
            warn!(plugin = %plugin.name, hook, rc, "plugin returned non-zero");
        }
        Ok(())
    }
}

fn get_hook<P, R>(
    store: &mut Store<()>,
    instance: &Instance,
    name: &str,
) -> Result<Option<TypedFunc<P, R>>>
where
    P: wasmtime::WasmParams,
    R: wasmtime::WasmResults,
{
    match instance.get_typed_func::<P, R>(store, name) {
        Ok(func) => Ok(Some(func)),
        Err(_) => Ok(None),
    }
}

fn write_memory(
    store: &mut Store<()>,
    memory: Memory,
    offset: usize,
    payload: &[u8],
) -> Result<()> {
    memory
        .write(store, offset, payload)
        .context("write payload to wasm memory")
}

fn req_for_blocking(req: &Request<Body>) -> RequestSnapshot {
    RequestSnapshot {
        method: req.method().to_string(),
        uri: req.uri().to_string(),
        headers: req
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).to_string(),
                )
            })
            .collect(),
    }
}

fn res_for_blocking(res: &Response<Body>) -> ResponseSnapshot {
    ResponseSnapshot {
        status: res.status().as_u16(),
        headers: res
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).to_string(),
                )
            })
            .collect(),
    }
}
