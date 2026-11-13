//! 应用共享状态：在接口处理函数与后台 worker 之间共享。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::catalog::Catalog;
use crate::config::Config;
use crate::db::Db;
use crate::media::Media;
use crate::providers::Providers;

/// 全局状态。字段都是 `Arc` 或轻量句柄，克隆开销很小，可以随请求传递。
#[derive(Clone)]
pub struct AppState {
    /// 运行配置。
    pub config: Arc<Config>,
    /// 数据库。
    pub db: Arc<Db>,
    /// 文件存储与签名地址。
    pub media: Arc<Media>,
    /// 四个模型与渲染器的供应商。
    pub providers: Providers,
    /// 能力目录。
    pub catalog: Arc<Catalog>,
    /// 与后台 worker 通信的句柄。
    pub worker: WorkerHandle,
}

/// 接口层通知工作线程的句柄：有新任务时唤醒，取消时触发对应令牌。
#[derive(Clone, Default)]
pub struct WorkerHandle {
    /// 唤醒调度循环，避免新任务等到下一次定时轮询。
    pub wake: Arc<Notify>,
    /// 本进程正在执行的任务及其取消令牌。
    pub running: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl WorkerHandle {
    /// 唤醒调度器，立即检查队列。
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// 请求停止任务：触发执行中的取消令牌，让正在等待的供应商调用尽快返回。
    ///
    /// 任务状态的变更（`cancelling`）由调用方在事务中完成；这里只负责通知。
    pub fn cancel(&self, job_id: &str) {
        if let Some(token) = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(job_id)
        {
            token.cancel();
        }
        self.wake();
    }

    /// 任务是否正由本进程执行。
    pub fn is_running(&self, job_id: &str) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(job_id)
    }
}
