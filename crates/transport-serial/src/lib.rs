//! 串口传输：只负责字节收发，不理解任何协议帧格式。

use async_trait::async_trait;
use hmi_domain::config::{ChannelConfig, Parity, SerialParams, TransportConfig};
use hmi_domain::link::{
    Link, LinkError, TransportError, TransportFactory, TransportKind, TransportTarget,
};
use std::any::Any;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 串口传输工厂。
#[derive(Debug, Default, Clone, Copy)]
pub struct SerialFactory;

#[async_trait]
impl TransportFactory for SerialFactory {
    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }

    async fn open(&self, channel: &ChannelConfig) -> Result<Box<dyn Link>, TransportError> {
        let TransportConfig::Serial(params) = &channel.transport else {
            return Err(TransportError::Unavailable(format!(
                "通道 {} 不是串口通道",
                channel.id
            )));
        };
        let link = open_with_hw_fallback(params).map_err(TransportError::Open)?;
        Ok(Box::new(link))
    }

    async fn targets(&self) -> Vec<TransportTarget> {
        list_serial_ports()
            .into_iter()
            .map(|p| TransportTarget {
                id: p.port_name.clone(),
                label: p.port_name,
                description: p.description,
                hw_id: p.hw_id,
            })
            .collect()
    }
}

/// 打开串口；失败且配置了硬件 ID 时，按 VID/PID 找回漂移后的 COM 号再试一次。
fn open_with_hw_fallback(params: &SerialParams) -> Result<SerialLink, String> {
    match SerialLink::open(params) {
        Ok(t) => Ok(t),
        Err(e) => {
            let Some(hw) = &params.hw_id else {
                return Err(e);
            };
            for p in list_serial_ports() {
                if p.hw_id.as_deref() == Some(hw.as_str()) && p.port_name != params.port {
                    tracing::info!(
                        from = %params.port,
                        to = %p.port_name,
                        "COM 号漂移，按硬件 ID 重新匹配"
                    );
                    let mut p2 = params.clone();
                    p2.port = p.port_name;
                    return SerialLink::open(&p2);
                }
            }
            Err(e)
        }
    }
}

/// 一个实例独占一个串口。
pub struct SerialLink {
    stream: tokio::sync::Mutex<tokio_serial::SerialStream>,
}

impl SerialLink {
    /// 打开串口。端口不存在/被占用返回 Err（字符串为可读原因）。
    pub fn open(params: &SerialParams) -> Result<Self, String> {
        let builder = tokio_serial::new(&params.port, params.baud_rate)
            .data_bits(to_data_bits(params.data_bits))
            .parity(to_parity(params.parity))
            .stop_bits(to_stop_bits(params.stop_bits))
            // 底层读节流；事务级超时由协议层通过 recv(timeout) 控制
            .timeout(Duration::from_millis(20));
        let stream = tokio_serial::SerialStream::open(&builder)
            .map_err(|e| format!("打开串口 {} 失败: {e}", params.port))?;
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

#[async_trait]
impl Link for SerialLink {
    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }

    async fn send(&self, bytes: &[u8]) -> Result<(), LinkError> {
        let mut stream = self.stream.lock().await;
        stream
            .write_all(bytes)
            .await
            .map_err(|e| LinkError::Io(e.to_string()))?;
        stream
            .flush()
            .await
            .map_err(|e| LinkError::Io(e.to_string()))
    }

    async fn recv(&self, max: usize, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut stream = self.stream.lock().await;
        let mut buf = vec![0u8; max.max(1)];
        match tokio::time::timeout(timeout, stream.read(&mut buf)).await {
            Ok(Ok(0)) => Err(LinkError::Closed),
            Ok(Ok(n)) => {
                buf.truncate(n);
                Ok(buf)
            }
            Ok(Err(e)) => Err(LinkError::Io(e.to_string())),
            Err(_) => Err(LinkError::Timeout),
        }
    }

    async fn drain(&self) {
        let mut stream = self.stream.lock().await;
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

    fn as_any(&self) -> &dyn Any {
        self
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

/// 串口描述（配置界面/命令行共用）。
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

    #[test]
    fn missing_port_is_reported() {
        let p = SerialParams {
            port: "COM_DOES_NOT_EXIST_999".into(),
            hw_id: None,
            baud_rate: 9600,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            inter_frame_gap_ms: 5,
        };
        assert!(SerialLink::open(&p).is_err());
    }
}
