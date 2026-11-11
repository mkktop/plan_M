//! 串口传输实现：tokio-serial 之上的"一问一答"事务。

use hmi_domain::config::{Parity, SerialChannelConfig};
use hmi_domain::transport::{BusError, BusTransport};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Duration;

use crate::frame;

/// 串口总线传输。一个实例独占一个串口。
pub struct SerialTransport {
    stream: tokio::sync::Mutex<tokio_serial::SerialStream>,
}

impl SerialTransport {
    /// 打开串口。端口不存在/被占用返回 Err（字符串为可读原因）。
    pub fn open(cfg: &SerialChannelConfig) -> Result<Self, String> {
        let builder = tokio_serial::new(&cfg.port, cfg.baud_rate)
            .data_bits(to_data_bits(cfg.data_bits))
            .parity(to_parity(cfg.parity))
            .stop_bits(to_stop_bits(cfg.stop_bits))
            .timeout(Duration::from_millis(50)); // 单次底层读的节流超时，事务级超时在外层
        let stream = tokio_serial::SerialStream::open(&builder)
            .map_err(|e| format!("打开串口 {} 失败: {e}", cfg.port))?;
        Ok(Self {
            stream: tokio::sync::Mutex::new(stream),
        })
    }
}

fn to_data_bits(bits: u8) -> serialport::DataBits {
    match bits {
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    }
}

fn to_parity(p: Parity) -> serialport::Parity {
    match p {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    }
}

fn to_stop_bits(b: u8) -> serialport::StopBits {
    match b {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    }
}

#[async_trait::async_trait]
impl BusTransport for SerialTransport {
    async fn transact(&self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, BusError> {
        let mut stream = self.stream.lock().await;
        let result = tokio::time::timeout(timeout, async {
            stream.write_all(request).await.map_err(io_err)?;
            stream.flush().await.map_err(io_err)?;

            // 按应答结构分步读取：先 3 字节帧头，推断总长后读余下
            let mut header = [0u8; 3];
            stream.read_exact(&mut header).await.map_err(io_err)?;
            let total = frame::expected_resp_len(&header)
                .ok_or_else(|| BusError::Io(format!("无法识别的应答帧头: {:02X?}", header)))?;
            let mut resp = header.to_vec();
            resp.resize(total, 0);
            stream.read_exact(&mut resp[3..]).await.map_err(io_err)?;
            Ok::<_, BusError>(resp)
        })
        .await;

        match result {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(e)) => {
                // I/O 错误（如 USB 拔出）→ 上层触发重连
                Err(e)
            }
            Err(_elapsed) => {
                // 超时：清掉可能已到的残字节，避免污染下一事务
                drain(&mut stream).await;
                Err(BusError::Timeout)
            }
        }
    }
}

fn io_err(e: std::io::Error) -> BusError {
    BusError::Io(e.to_string())
}

/// 非阻塞地丢弃输入缓冲中的残字节（超时事务后防止残帧污染下一事务）。
async fn drain(stream: &mut tokio_serial::SerialStream) {
    let mut buf = [0u8; 256];
    // 给残帧一点到达时间，然后短超时循环读到无数据为止
    tokio::time::sleep(Duration::from_millis(30)).await;
    for _ in 0..8 {
        match tokio::time::timeout(Duration::from_millis(60), stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => continue,
            _ => break,
        }
    }
}

/// 枚举本机串口，附带 USB 硬件 ID（用于 COM 号漂移匹配）。
pub fn list_serial_ports() -> Vec<SerialPortDesc> {
    let Ok(ports) = serialport::available_ports() else {
        return Vec::new();
    };
    ports
        .into_iter()
        .map(|p| {
            let (hw_id, desc) = match &p.port_type {
                serialport::SerialPortType::UsbPort(usb) => (
                    Some(format!("VID_{:04X}&PID_{:04X}", usb.vid, usb.pid)),
                    usb.product.clone().unwrap_or_else(|| "USB 串口".into()),
                ),
                serialport::SerialPortType::BluetoothPort => (None, "蓝牙串口".into()),
                serialport::SerialPortType::PciPort => (None, "PCI 串口".into()),
                _ => (None, "未知类型".into()),
            };
            SerialPortDesc {
                port_name: p.port_name,
                hw_id,
                description: desc,
            }
        })
        .collect()
}

/// 串口描述（wire 层与配置界面共用）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortDesc {
    pub port_name: String,
    pub hw_id: Option<String>,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_mapping() {
        assert_eq!(to_parity(Parity::None), serialport::Parity::None);
        assert_eq!(to_parity(Parity::Even), serialport::Parity::Even);
    }
}
