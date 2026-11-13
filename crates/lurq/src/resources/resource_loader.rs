use std::{collections::HashMap, io::ErrorKind, path::PathBuf, sync::Arc};

use parking_lot::RwLock;
use ureq::{RequestBuilder, typestate::WithoutBody};

use crate::resources::{
  ResourceConfig, ResourceError, core::LoadResourceResult, resource_cache::ResourceCache, thread_pool::ThreadPool,
};

type PendingMap = Arc<RwLock<HashMap<Arc<str>, ResourceConfig>>>;
type ResolvedMap = Arc<RwLock<HashMap<Arc<str>, Arc<Vec<u8>>>>>;
type ErrorMap = Arc<RwLock<HashMap<Arc<str>, ResourceError>>>;

#[derive(Default)]
pub struct LoaderConfig {
  pub headers: Vec<(String, String)>,
}

pub struct ResourceLoader {
  asset_root: Option<PathBuf>,
  cache: ResourceCache,
  pool: ThreadPool,
  pending: PendingMap,
  resolved: ResolvedMap,
  errors: ErrorMap,
  config: Arc<LoaderConfig>,
}

impl Default for ResourceLoader {
  fn default() -> Self {
    Self::new()
  }
}

impl ResourceLoader {
  pub fn new() -> Self {
    Self {
      asset_root: None,
      cache: ResourceCache::default(),
      pool: ThreadPool::new(4),
      pending: Arc::new(RwLock::new(HashMap::default())),
      resolved: Arc::new(RwLock::new(HashMap::default())),
      errors: Arc::new(RwLock::new(HashMap::default())),
      config: Arc::new(LoaderConfig::default()),
    }
  }

  pub fn set_root(&mut self, root: PathBuf) {
    self.asset_root = Some(root)
  }

  pub fn reset_root(&mut self) {
    self.asset_root = None
  }

  pub fn evict_expired(&self) {
    self.cache.evict_expired();
  }

  pub fn flush_cache(&self) {
    self.cache.flush();
    self.resolved.write().clear();
  }

  pub fn load_resource(&self, path: &Arc<str>, config: Option<ResourceConfig>) -> LoadResourceResult {
    if let Some(c) = self.get(path) {
      return c;
    }

    let config = config.unwrap_or_default();

    {
      let mut w = self.pending.write();
      w.insert(path.clone(), config);
    }

    if path.starts_with("http://") || path.starts_with("https://") {
      self.load_remote(path.clone())
    } else {
      self.load_local(path.clone())
    }

    LoadResourceResult::Pending
  }

  pub fn get(&self, path: &Arc<str>) -> Option<LoadResourceResult> {
    {
      let r = self.errors.read();
      if let Some(e) = r.get(path) {
        return Some(LoadResourceResult::Error(e.clone()));
      }
    }

    if let Some(c) = self.cache.get(path) {
      return Some(LoadResourceResult::Loaded(c.data.clone()));
    }

    let mut w = self.resolved.write();
    w.remove(path).map(LoadResourceResult::Loaded)
  }

  pub fn set_config(&mut self, config: LoaderConfig) {
    self.config = Arc::new(config);
  }

  fn resolve_pending(
    pending: PendingMap,
    resolved: ResolvedMap,
    errors: ErrorMap,
    cache: ResourceCache,
    path: Arc<str>,
    result: LoadResourceResult,
  ) {
    let config = {
      let mut w = pending.write();
      let Some(c) = w.remove(&path) else {
        return;
      };
      c
    };

    match result {
      LoadResourceResult::Loaded(ref data) => {
        if config.ttl == 0 {
          let mut w = resolved.write();
          w.insert(path, data.clone());
        } else {
          cache.insert(&path, data.clone(), config);
        }
      }
      LoadResourceResult::Error(err) => {
        let mut w = errors.write();
        w.insert(path, err);
      }
      _ => {}
    }
  }

  fn load_remote(&self, path: Arc<str>) {
    let pending = self.pending.clone();
    let cache = self.cache.clone();
    let resolved = self.resolved.clone();
    let errors = self.errors.clone();
    let config = self.config.clone();

    fn build_req(path: &Arc<str>, config: Arc<LoaderConfig>) -> RequestBuilder<WithoutBody> {
      let mut c = ureq::get(path.as_ref());
      for x in &config.headers {
        c = c.header(x.0.clone(), x.1.clone());
      }
      c
    }

    self.pool.execute(move || {
      let retries = {
        let r = pending.read();
        r.get(&path).map(|c| c.retries).unwrap_or(0)
      };

      let mut last_err = None;
      for _ in 0..=retries {
        match build_req(&path, config.clone()).call() {
          Ok(resp) => match resp.into_body().read_to_vec() {
            Ok(bytes) => {
              let result = LoadResourceResult::Loaded(Arc::new(bytes));
              Self::resolve_pending(pending, resolved, errors, cache, path, result);
              return;
            }
            Err(e) => last_err = Some(e),
          },
          Err(e) => last_err = Some(e),
        }
      }

      let result = if let Some(e) = last_err {
        match e {
          ureq::Error::StatusCode(404) => LoadResourceResult::Error(ResourceError::NotFound),
          ureq::Error::StatusCode(code) => LoadResourceResult::Error(ResourceError::NetworkError(code)),
          _ => LoadResourceResult::Error(ResourceError::Unknown(e.to_string())),
        }
      } else {
        LoadResourceResult::Error(ResourceError::Unknown("failed to read response body".into()))
      };

      Self::resolve_pending(pending, resolved, errors, cache, path, result);
    })
  }

  fn load_local(&self, path: Arc<str>) {
    let path_buf = self.normalize_path(&path);
    let pending = self.pending.clone();
    let cache = self.cache.clone();
    let resolved = self.resolved.clone();
    let errors = self.errors.clone();

    self.pool.execute(move || {
      let retries = {
        let r = pending.read();
        r.get(&path).map(|c| c.retries).unwrap_or(0)
      };

      let mut last_err = None;
      for _ in 0..=retries {
        match std::fs::read(&path_buf) {
          Ok(bytes) => {
            let result = LoadResourceResult::Loaded(Arc::new(bytes));
            Self::resolve_pending(pending, resolved, errors, cache, path, result);
            return;
          }
          Err(e) => last_err = Some(e),
        }
      }

      let result = match last_err.unwrap() {
        e if e.kind() == ErrorKind::NotFound => LoadResourceResult::Error(ResourceError::NotFound),
        e if e.raw_os_error().is_some() => LoadResourceResult::Error(ResourceError::os_err(e.raw_os_error().unwrap())),
        e => LoadResourceResult::Error(ResourceError::Unknown(e.to_string())),
      };

      Self::resolve_pending(pending, resolved, errors, cache, path, result);
    })
  }

  fn normalize_path(&self, path: &Arc<str>) -> PathBuf {
    let mut root = self.asset_root.as_ref().cloned().unwrap_or(PathBuf::new());

    root.push(path.to_string());

    root
  }
}
