use anyhow::{Context, Result};
use hudsucker::{Body, hyper::Request, hyper::Response};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::time::{Duration, timeout};
use tracing::{debug, info, warn};
use wasmtime::{Engine, Instance, Memory, Module, Store, TypedFunc};

#[derive(Clone)]
pub struct WasmPluginHost {
    engine: Engine,
    plugins: Vec<WasmPlugin>,
    timeout_ms: u64,
    max_failures: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RequestMutation {
    #[serde(default)]
    pub add_headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ResponseMutation {
    #[serde(default)]
    pub add_headers: Vec<(String, String)>,
    #[serde(default)]
    pub set_status: Option<u16>,
    #[serde(default)]
    pub replace_body: Option<String>,
}

#[derive(Clone)]
struct WasmPlugin {
    name: String,
    module: Module,
    failures: Arc<AtomicU64>,
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
    pub fn load(plugin_dir: &Path, timeout_ms: u64, max_failures: u64) -> Result<Self> {
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
                    plugins.push(WasmPlugin {
                        name,
                        module,
                        failures: Arc::new(AtomicU64::new(0)),
                    });
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
            max_failures,
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

    pub async fn eval_request_mutations(&self, req: &Request<Body>) -> Result<RequestMutation> {
        let this = self.clone();
        let snapshot = req_for_blocking(req);
        let task =
            tokio::task::spawn_blocking(move || this.eval_request_mutations_blocking(snapshot));
        match timeout(Duration::from_millis(self.timeout_ms), task).await {
            Ok(join_res) => join_res?,
            Err(_) => anyhow::bail!("wasm request mutation timeout: {}ms", self.timeout_ms),
        }
    }

    pub async fn eval_response_mutations(&self, res: &Response<Body>) -> Result<ResponseMutation> {
        let this = self.clone();
        let snapshot = res_for_blocking(res);
        let task =
            tokio::task::spawn_blocking(move || this.eval_response_mutations_blocking(snapshot));
        match timeout(Duration::from_millis(self.timeout_ms), task).await {
            Ok(join_res) => join_res?,
            Err(_) => anyhow::bail!("wasm response mutation timeout: {}ms", self.timeout_ms),
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

    fn eval_request_mutations_blocking(
        &self,
        snapshot: RequestSnapshot,
    ) -> Result<RequestMutation> {
        let payload = serde_json::to_vec(&snapshot)?;
        let mut out = RequestMutation::default();
        for plugin in &self.plugins {
            let failures = plugin.failures.load(Ordering::Relaxed);
            if failures >= self.max_failures {
                continue;
            }
            match self.invoke_mut_hook(plugin, "on_http_request_mut", &payload) {
                Ok(Some(bytes)) => {
                    if let Ok(mut action) = serde_json::from_slice::<RequestMutation>(&bytes) {
                        out.add_headers.append(&mut action.add_headers);
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    let n = plugin.failures.fetch_add(1, Ordering::Relaxed) + 1;
                    warn!(
                        plugin = %plugin.name,
                        failures = n,
                        max_failures = self.max_failures,
                        error = %err,
                        "request mutation hook failed"
                    );
                }
            }
        }
        Ok(out)
    }

    fn eval_response_mutations_blocking(
        &self,
        snapshot: ResponseSnapshot,
    ) -> Result<ResponseMutation> {
        let payload = serde_json::to_vec(&snapshot)?;
        let mut out = ResponseMutation::default();
        for plugin in &self.plugins {
            let failures = plugin.failures.load(Ordering::Relaxed);
            if failures >= self.max_failures {
                continue;
            }
            match self.invoke_mut_hook(plugin, "on_http_response_mut", &payload) {
                Ok(Some(bytes)) => {
                    if let Ok(mut action) = serde_json::from_slice::<ResponseMutation>(&bytes) {
                        out.add_headers.append(&mut action.add_headers);
                        if out.set_status.is_none() {
                            out.set_status = action.set_status.take();
                        }
                        if out.replace_body.is_none() {
                            out.replace_body = action.replace_body.take();
                        }
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    let n = plugin.failures.fetch_add(1, Ordering::Relaxed) + 1;
                    warn!(
                        plugin = %plugin.name,
                        failures = n,
                        max_failures = self.max_failures,
                        error = %err,
                        "response mutation hook failed"
                    );
                }
            }
        }
        Ok(out)
    }

    fn broadcast(&self, hook: &str, payload: &[u8]) -> Result<()> {
        for plugin in &self.plugins {
            let failures = plugin.failures.load(Ordering::Relaxed);
            if failures >= self.max_failures {
                debug!(
                    plugin = %plugin.name,
                    failures,
                    max_failures = self.max_failures,
                    "plugin disabled by failure budget"
                );
                continue;
            }
            if let Err(err) = self.invoke_hook(plugin, hook, payload) {
                let n = plugin.failures.fetch_add(1, Ordering::Relaxed) + 1;
                warn!(
                    plugin = %plugin.name,
                    hook,
                    failures = n,
                    max_failures = self.max_failures,
                    error = %err,
                    "plugin hook failed"
                );
            }
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

    fn invoke_mut_hook(
        &self,
        plugin: &WasmPlugin,
        hook: &str,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let mut store = Store::new(&self.engine, ());
        let instance = Instance::new(&mut store, &plugin.module, &[])
            .with_context(|| format!("instantiate {}", plugin.name))?;

        let Some(hook_fn) = get_hook::<(i32, i32), i64>(&mut store, &instance, hook)? else {
            return Ok(None);
        };
        let Some(alloc_fn) = get_hook::<i32, i32>(&mut store, &instance, "alloc")? else {
            return Ok(None);
        };
        let Some(dealloc_fn) = get_hook::<(i32, i32), ()>(&mut store, &instance, "dealloc")? else {
            return Ok(None);
        };
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("memory export not found")?;

        let in_ptr = alloc_fn.call(&mut store, payload.len() as i32)?;
        write_memory(&mut store, memory, in_ptr as usize, payload)?;
        let packed = hook_fn.call(&mut store, (in_ptr, payload.len() as i32))?;
        dealloc_fn.call(&mut store, (in_ptr, payload.len() as i32))?;
        if packed == 0 {
            return Ok(None);
        }
        let out_ptr = ((packed >> 32) & 0xffff_ffff) as i32;
        let out_len = (packed & 0xffff_ffff) as i32;
        if out_ptr <= 0 || out_len <= 0 {
            return Ok(None);
        }
        let mut out = vec![0u8; out_len as usize];
        memory
            .read(&store, out_ptr as usize, &mut out)
            .context("read mutation payload from wasm memory")?;
        dealloc_fn.call(&mut store, (out_ptr, out_len))?;
        Ok(Some(out))
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
