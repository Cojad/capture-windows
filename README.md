# capture-windows

A Rust proof-of-concept hardware monitoring agent for Checkmate.

I created this version because I had already set up Checkmate for several websites and Linux infrastructures, but I needed Windows infrastructure support immediately. I didn’t want to bother installing multiple monitoring suites just to find one that suited my needs. So, I decided I only needed a simple CPU/memory/disk usage monitoring agent. How hard could it be, since we can see those numbers in Task Manager and Explorer? The best part is, I have a handful of AI agents to assist me. 😉

With the help of ChatGPT, I got a working CPU and memory usage monitor running in about an hour (half of that time was just setting up the Rust build environment). That encouraged me to continue adding more features. After around 4–5 hours, I had a version that correctly reports CPU usage, memory usage, system drive usage, and OS name. Although it's missing features like authorization checks, CPU temperature, and network usage, it already exceeds my initial goals. So, I decided to share it, especially since the official Windows release roadmap is still about a month out.

> ⚠️ **Warning:** This is just a proof of concept — not even close to beta or production-ready. Most importantly, it lacks any form of authorization checking. Please use it with caution. I take no responsibility for the use of this monitoring agent.

There’s no settings file. The port is controlled by the `PORT` environment variable, defaulting to `59232`. When the program starts, it opens a console window that logs every incoming request.

**Default endpoint:**  
http://0.0.0.0:59232/api/v1/metrics  
(No authorization is required — you can fill in any random text on the Checkmate configuration page.)

---

### Rust Windows Artifacts (amd64)

> Note: Some metrics are not supported on Windows.

| Metric                  | Status                                |
|-------------------------|----------------------------------------|
| CPU                    | ❌ Not implemented                     |
| CPU usage              | ✅ Implemented                         |
| CPU temperature        | ❌ Not supported yet                   |
| CPU current frequency  | ✅ Using PDH (Performance Data Helper) |
| Disk                  | ❌ Not implemented                     |
| System disk usage      | ✅ Implemented                         |
| Other disk usage       | ❌ Not implemented                     |
| Disk filtering         | ❌ Linux-only                          |
| Docker                 | ❌ Not considered                      |
| Host                   | ✅ Implemented                         |
| OS pretty name         | ✅ Implemented                         |
| Memory                 | ✅ Cross-platform (via gopsutil)       |
| Network                | ❌ Not considered                      |
| SMART metrics          | ❌ Not considered                      |
| SMART (via smartctl)   | ❌ Unix-only                           |
