//! Windows-only Metrics Server：current_frequency 透過 PDH 讀取
use axum::{
    body::Body,
    http::StatusCode,
    middleware::{from_fn, Next},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use chrono::Local;
use serde::Serialize;
use std::{
    net::SocketAddr,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{CpuRefreshKind, RefreshKind, System};
use tokio::net::TcpListener;
use windows::{
    core::{w, PCWSTR},
    Win32::System::Performance::{
        PdhAddCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_LARGE,
    },
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
// use windows::Win32::Foundation::BOOL;

/* ---------- 資料結構 ---------- */

#[derive(Serialize)]
struct CPUData {
    physical_core: usize,
    logical_core: usize,
    frequency: u64,
    current_frequency: Option<u64>,
    temperature_c: Option<f32>,
    free_percent: f32,
    usage_percent: f32,
}

#[derive(Serialize)]
struct MemoryData {
    total_bytes: u64,
    available_bytes: u64,
    used_bytes: u64,
    usage_percent: f32,
}

#[derive(Serialize)]
struct DiskData {
    device: String,
    total_bytes: Option<u64>,
    free_bytes: Option<u64>,
    used_bytes: Option<u64>,
    usage_percent: Option<f32>,
    // 其餘欄位暫時省略
}

#[derive(Serialize)]
struct HostData {
    os: String,
    platform: String,
    kernel_version: String,
    pretty_name: String,
}

#[derive(Serialize, Default)]
struct NetData {
    name: String,
    bytes_sent: u64,
    bytes_recv: u64,
    packets_sent: u64,
    packets_recv: u64,
    err_in: u64,
    err_out: u64,
    drop_in: u64,
    drop_out: u64,
    fifo_in: u64,
    fifo_out: u64,
}

#[derive(Serialize)]
struct CaptureMeta {
    version: String,
    mode: String,
}

#[derive(Serialize)]
struct MetricError {
    metric: Vec<String>,
    err: String,
}

#[derive(Serialize)]
struct AllMetrics {
    data: AllData,
    capture: CaptureMeta,
    errors: Vec<MetricError>,
}

#[derive(Serialize)]
struct AllData {
    cpu: CPUData,
    memory: MemoryData,
    disk: Vec<DiskData>,
    host: HostData,
    net: Vec<NetData>,
}

/* ---------- PDH 讀取 CPU 目前頻率 ---------- */

fn query_current_freq_mhz() -> Result<u64, String> {
    unsafe {
        let mut query: isize = 0;
        let status: u32 = PdhOpenQueryW(PCWSTR::null(), 0, &mut query);
        if status != 0 {
            return Err(format!("PdhOpenQueryW failed: {status}"));
        }

        let mut counter: isize = 0;
        let path = w!("\\Processor Information(0,0)\\Processor Frequency");
        let status: u32 = PdhAddCounterW(query, path, 0, &mut counter);
        if status != 0 {
            PdhCloseQuery(query);
            return Err(format!("PdhAddCounterW failed: {status}"));
        }

        PdhCollectQueryData(query);
        thread::sleep(Duration::from_millis(120));
        PdhCollectQueryData(query);

        let mut val: PDH_FMT_COUNTERVALUE = std::mem::zeroed();
        let status: u32 =
            PdhGetFormattedCounterValue(counter, PDH_FMT_LARGE, None, &mut val);
        PdhCloseQuery(query);

        if status != 0 {
            return Err(format!("PdhGetFormattedCounterValue failed: {status}"));
        }
        Ok(val.Anonymous.largeValue as u64)
    }
}

/* ---------- Apache-style Middleware ---------- */

async fn log_apache(req: axum::http::Request<Body>, next: Next) -> impl IntoResponse {
    let started = Instant::now();
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let ip = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    let resp = next.run(req).await;

    println!(
        "{ip} - - [{}] \"{} {} HTTP/1.1\" {} {}ms",
        Local::now().format("%d/%b/%Y:%H:%M:%S %z"),
        method,
        path,
        resp.status().as_u16(),
        started.elapsed().as_millis()
    );
    resp
}

/* ---------- 路由 ---------- */

async fn all_metrics() -> impl IntoResponse {
    let mut errors: Vec<MetricError> = Vec::new();

    let cpu = gather_cpu(&mut errors);

    Json(AllMetrics {
        data: AllData {
            cpu,
            memory: gather_memory(),
            disk: gather_disk(),
            host: gather_host(),
            net: gather_net(),
        },
        capture: CaptureMeta {
            version: "1.2.0".into(),
            mode: "debug".into(),
        },
        errors,
    })
}

async fn cpu_metrics() -> impl IntoResponse {
    Json(gather_cpu(&mut Vec::new()))
}
async fn memory_metrics() -> impl IntoResponse {
    Json(gather_memory())
}
async fn null_response() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "data": null })))
}

/* ---------- 指標蒐集 ---------- */

fn gather_cpu(errors: &mut Vec<MetricError>) -> CPUData {
    // 讀系統靜態頻率與使用率
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
    );
    sys.refresh_cpu_specifics(CpuRefreshKind::everything());
    thread::sleep(Duration::from_millis(250));
    sys.refresh_cpu_specifics(CpuRefreshKind::everything());

    let usage = sys.global_cpu_usage();
    let base_freq = sys
        .cpus()
        .iter()
        .map(|c| c.frequency())
        .max()
        .unwrap_or(0);

    // 讀取即時頻率
    let current_freq = match query_current_freq_mhz() {
        Ok(v) => Some(v),
        Err(e) => {
            errors.push(MetricError {
                metric: vec!["cpu.current_frequency".into()],
                err: e,
            });
            None
        }
    };

    // 溫度仍無法取得
    errors.push(MetricError {
        metric: vec!["cpu.temperature".into()],
        err: "unable to read CPU temperature".into(),
    });

    // errors.push(MetricError {
    //     metric: vec!["cpu.current_frequency".into()],
    //     err: "unable to read CPU frequency".into(),
    // });

    CPUData {
        physical_core: System::physical_core_count().unwrap_or(0),
        logical_core: sys.cpus().len(),
        frequency: base_freq,
        current_frequency: current_freq,
        temperature_c: None,
        free_percent: 1.0 - usage / 100.0,
        usage_percent: usage / 100.0,
    }
}

fn gather_memory() -> MemoryData {
    let mut sys = System::new();
    sys.refresh_memory();
    let total = sys.total_memory();
    let avail = sys.available_memory();
    let used = total.saturating_sub(avail);

    MemoryData {
        total_bytes: total * 1024,
        available_bytes: avail * 1024,
        used_bytes: used * 1024,
        usage_percent: used as f32 / total as f32,
    }
}

fn gather_disk() -> Vec<DiskData> {
    // 只示範 C:\
    let path = w!("C:\\");
    let mut free:    u64 = 0;
    let mut total:   u64 = 0;
    let mut _unused: u64 = 0;

    // 回傳 Result<(), Error>
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(path.as_ptr()),
            Some(&mut _unused),      // caller 可用空間（未用）
            Some(&mut total),        // 總容量
            Some(&mut free),         // 剩餘容量
        )
    };

    if ok.is_err() || total == 0 {
        // 失敗 → 交由上層決定是否加入 errors
        return Vec::new();
    }

    let used = total.saturating_sub(free);
    let percent = used as f32 / total as f32;

    vec![DiskData {
        device: "C:\\".into(),
        total_bytes: Some(total),
        free_bytes: Some(free),
        used_bytes: Some(used),
        usage_percent: Some(percent),
    }]
}

fn gather_host() -> HostData {
    // 取得漂亮名稱；可能回傳 None
    let pretty_name = sysinfo::System::long_os_version().unwrap_or_else(|| "unknown".into());

    // platform = pretty_name，但把前綴 "Windows " 去掉
    let platform = {
        // 先移除大小寫皆為 "windows " 的前綴
        let lower = pretty_name.to_lowercase();
        if lower.starts_with("windows ") {
            // 長度相同，可安全依原字串   取切片
            pretty_name.chars().skip(8).collect()
        } else {
            pretty_name.clone()
        }
    };

    HostData {
        os: sysinfo::System::name().unwrap_or_else(|| "unknown".into()),
        platform,
        kernel_version: sysinfo::System::kernel_version().unwrap_or_else(|| "unknown".into()),
        pretty_name,
    }
}


fn gather_net() -> Vec<NetData> {
    vec![
        NetData {
            name: "lo".into(),
            ..Default::default()
        },
        NetData {
            name: "eth0".into(),
            ..Default::default()
        },
    ]
}

/* ---------- 入口 ---------- */
#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "59232".into())
        .parse()
        .unwrap_or(59232);

    let app = Router::new()
        .route("/api/v1/metrics", get(all_metrics))
        .route("/api/v1/metrics/cpu", get(cpu_metrics))
        .route("/api/v1/metrics/memory", get(memory_metrics))
        .fallback(get(null_response))
        .layer(from_fn(log_apache));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("🚀  listening on http://{addr}");
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
