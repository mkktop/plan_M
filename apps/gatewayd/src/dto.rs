//! gateway 领域视图 → wire DTO 的唯一映射层。
//!
//! `hmi-gateway` 只产出领域类型（`Snapshot` / `InstalledPackage` / `InstalledUiPlugin`），
//! 本模块负责翻译成 WS 契约；gatewayd 的 WS 处理器只调用这里。

use hmi_domain::driver::Driver;
use hmi_gateway::{ChannelStatus, DeviceStatus, GatewayService, Snapshot};
use hmi_packaging::{BindingSpec as PkgBindingSpec, InstalledPackage, InstalledUiPlugin};
use hmi_wire::{
    BindingDeclDto, BindingSpecDto, ChannelDto, DeviceDto, DevicePageComponentDto, DevicePageDto,
    DriverInfoDto, PackageDto, PackageUiDto, PackageWidgetDto, PointDto, SnapshotData,
    UiDevicePanelDto, UiPageDto, UiPluginDto, UiWidgetDto, WidgetRectDto, WidgetSizeDto,
};

/// 驱动能力 → DTO（Welcome 用）。
pub fn driver_infos(service: &GatewayService) -> Vec<DriverInfoDto> {
    let mut list: Vec<DriverInfoDto> = service
        .drivers()
        .iter()
        .map(|d: &Arc<dyn Driver>| DriverInfoDto {
            id: d.id().to_string(),
            name: d.name().to_string(),
            transports: d.transports().to_vec(),
        })
        .collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

/// 领域快照 → WS 快照。
pub fn snapshot_data(view: Snapshot) -> SnapshotData {
    SnapshotData {
        channels: view
            .channels
            .into_iter()
            .map(|c: ChannelStatus| ChannelDto {
                config: c.config,
                state: c.state,
                config_error: c.config_error,
            })
            .collect(),
        devices: view
            .devices
            .into_iter()
            .map(|d: DeviceStatus| DeviceDto {
                config: d.config,
                online: d.online,
                config_error: d.config_error,
            })
            .collect(),
        packages: view.packages.iter().map(package_dto).collect(),
        tags: view.tags,
    }
}

/// 已安装包列表 → DTO 列表。
pub fn package_dtos(list: &[InstalledPackage]) -> Vec<PackageDto> {
    list.iter().map(package_dto).collect()
}

/// 已安装包 → DTO（快照与包列表共用）。
pub fn package_dto(p: &InstalledPackage) -> PackageDto {
    PackageDto {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        vendor: p.manifest.vendor.clone(),
        version: p.manifest.version.to_string(),
        protocol: p.manifest.protocol.clone(),
        description: p.manifest.description.clone(),
        ui: PackageUiDto {
            pages: p
                .manifest
                .ui
                .pages
                .iter()
                .map(|page| DevicePageDto {
                    id: page.id.clone(),
                    title: page.title.clone(),
                    icon: page.icon.clone(),
                    components: page
                        .components
                        .iter()
                        .map(|comp| DevicePageComponentDto {
                            id: comp.id.clone(),
                            kind: comp.kind.clone(),
                            title: comp.title.clone(),
                            bind: comp
                                .bind
                                .iter()
                                .map(|(slot, spec)| (slot.clone(), binding_spec_dto(spec)))
                                .collect(),
                            rect: comp.rect.map(|r| WidgetRectDto {
                                x: r.x,
                                y: r.y,
                                w: r.w,
                                h: r.h,
                            }),
                            config: comp.config.clone(),
                        })
                        .collect(),
                })
                .collect(),
            widgets: p
                .manifest
                .ui
                .widgets
                .iter()
                .map(|w| PackageWidgetDto {
                    id: w.id.clone(),
                    title: w.title.clone(),
                    description: w.description.clone(),
                    entry: w.entry.clone(),
                    size: WidgetSizeDto {
                        w: w.size.w,
                        h: w.size.h,
                        min_w: w.size.min_w,
                        min_h: w.size.min_h,
                    },
                    bindings: w
                        .bindings
                        .iter()
                        .map(|(slot, b)| {
                            (
                                slot.clone(),
                                hmi_wire::WidgetBindingDto {
                                    kind: b.kind.clone(),
                                    many: b.many,
                                    write: b.write,
                                    required: b.required,
                                },
                            )
                        })
                        .collect(),
                    config: w.config.clone(),
                    permissions: w.permissions.clone(),
                })
                .collect(),
        },
        points: p
            .points
            .points
            .iter()
            .map(|pt| PointDto {
                id: pt.id.clone(),
                name: pt.name.clone(),
                address: pt.address.clone(),
                area: pt.address.as_register().map(|r| r.area),
                data_type: pt.data_type,
                byte_order: pt.byte_order,
                access: pt.access,
                scale: pt.scale,
                offset: pt.offset,
                unit: pt.unit.clone(),
                poll_group: pt.poll_group.clone(),
                bits: pt
                    .bits
                    .iter()
                    .map(|(bit, b)| hmi_wire::BitDto {
                        bit: *bit,
                        name: b.name.clone(),
                        on: b.on.clone(),
                        off: b.off.clone(),
                    })
                    .collect(),
                min: pt.min,
                max: pt.max,
                step: pt.step,
                value_enum: pt
                    .value_enum
                    .iter()
                    .map(|e| hmi_wire::EnumValueDto {
                        value: e.value,
                        label: e.label.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// 已安装 UI 插件列表 → DTO 列表。
pub fn ui_plugin_dtos(list: &[InstalledUiPlugin]) -> Vec<UiPluginDto> {
    list.iter().map(ui_plugin_dto).collect()
}

/// 已安装 UI 插件 → DTO。
pub fn ui_plugin_dto(p: &InstalledUiPlugin) -> UiPluginDto {
    let m = &p.manifest;
    UiPluginDto {
        id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.to_string(),
        description: m.description.clone(),
        author: m.author.clone(),
        sdk_version: m.sdk_version,
        permissions: m.permissions.clone(),
        entry: m.entry.clone(),
        widgets: m
            .widgets
            .iter()
            .map(|w| UiWidgetDto {
                id: w.id.clone(),
                name: w.name.clone(),
                description: w.description.clone(),
                default_w: w.default_w,
                default_h: w.default_h,
            })
            .collect(),
        pages: m
            .pages
            .iter()
            .map(|p| UiPageDto {
                id: p.id.clone(),
                title: p.title.clone(),
                icon: p.icon.clone(),
            })
            .collect(),
        device_panels: m
            .device_panels
            .iter()
            .map(|p| UiDevicePanelDto {
                id: p.id.clone(),
                title: p.title.clone(),
            })
            .collect(),
    }
}

/// 槽位绑定规格 → DTO。
fn binding_spec_dto(spec: &PkgBindingSpec) -> BindingSpecDto {
    match spec {
        PkgBindingSpec::One(b) => BindingSpecDto::One(binding_dto(b)),
        PkgBindingSpec::Many(v) => BindingSpecDto::Many(v.iter().map(binding_dto).collect()),
    }
}

fn binding_dto(b: &hmi_packaging::BindingDecl) -> BindingDeclDto {
    BindingDeclDto {
        point: b.point.clone(),
        bit: b.bit,
        tag: b.tag.clone(),
    }
}

use std::sync::Arc;
