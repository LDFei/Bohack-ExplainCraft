//! 轻量场景导演协议与离线矢量编译器。模型计划是数据，不是可执行程序。
use crate::providers::{ProviderError, ProviderResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenePlan {
    pub objective: String,
    pub visual_concept: String,
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    pub events: Vec<Event>,
    pub final_state: String,
    pub style: String,
    pub aspect_ratio: String,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub palette: Option<Palette>,
}

/// 自定义风格仍由受控色值表达，不能通过 CSS 引入外部资源。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub muted: String,
}

/// 归一化区域内的可组合原语；path 用本对象 0～100 的局部坐标，plot 使用实际数据。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub region: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub path: String,
    pub points: Vec<[f64; 2]>,
    pub count: u32,
    pub seed: u32,
    pub value: f64,
    pub unit: String,
    pub color: String,
    pub x_range: [f64; 2],
    pub y_range: [f64; 2],
    pub x_label: String,
    pub y_label: String,
    /// 数值直接绑定图表的真实数据点，宿主据同一时间轴驱动，不能生成伪插值。
    pub sample_from: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    pub from: String,
    pub to: String,
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub entity_id: String,
    pub action: String,
    pub start_ms: i64,
    pub duration_ms: i64,
    pub to: Change,
    pub reasoning: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Change {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub value: Option<f64>,
    pub text: Option<String>,
    pub opacity: Option<f64>,
    pub scale: Option<f64>,
    pub color: Option<String>,
    pub easing: String,
}
/// 模型提供的事实始终待核实；来源文字不能自动升级为「已验证」。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub statement: String,
    pub evidence: String,
}

/// 从旧视觉规格兼容读取新协议，不修改数据库表或通信协议。
pub fn from_prompt(prompt: &str) -> Option<ScenePlan> {
    let value: serde_json::Value = serde_json::from_str(prompt).ok()?;
    serde_json::from_value(value.get("scene_plan")?.clone()).ok()
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|c| c.is_ascii_hexdigit())
}

impl ScenePlan {
    /// 拒绝无知识对象、空洞动作、未知对象、不合理时间与越界数据；不把文字数量当动画能力。
    pub fn validate(&self, duration_ms: i64) -> ProviderResult<()> {
        let bad = || ProviderError::invalid("场景计划不完整或越界，请修复当前镜头。");
        if self.objective.trim().is_empty()
            || self.visual_concept.trim().is_empty()
            || self.final_state.trim().is_empty()
            || !["16:9", "9:16", "1:1"].contains(&self.aspect_ratio.as_str())
            || self.entities.is_empty()
            || self.entities.len() > 48
            || self.events.is_empty()
            || self.events.len() > 120
            || self.relationships.len() > 80
            || self.claims.len() > 30
        {
            return Err(bad());
        }
        let mut ids = HashSet::new();
        if self.palette.as_ref().is_some_and(|p| {
            [&p.background, &p.foreground, &p.accent, &p.muted]
                .iter()
                .any(|c| !color(c))
        }) {
            return Err(bad());
        }
        let mut graphic = false;
        for e in &self.entities {
            if e.id.is_empty()
                || e.id.len() > 80
                || !ids.insert(e.id.as_str())
                || !["primary", "secondary"].contains(&e.region.as_str())
                || ![
                    "path",
                    "ellipse",
                    "particles",
                    "plot",
                    "bars",
                    "vector",
                    "number",
                    "text",
                    "rect",
                ]
                .contains(&e.kind.as_str())
                || [e.x, e.y, e.width, e.height]
                    .iter()
                    .any(|v| !v.is_finite() || *v < 0. || *v > 1.)
                || e.width <= 0.
                || e.height <= 0.
                || e.x + e.width > 1.001
                || e.y + e.height > 1.001
                || e.label.chars().count() > 50
                || e.unit.chars().count() > 20
                || e.path.len() > 12000
                || e.points.len() > 600
                || e.count > 160
                || !e.value.is_finite()
                || (!e.color.is_empty() && !color(&e.color))
            {
                return Err(bad());
            }
            if e.kind == "path"
                && (e.path.is_empty()
                    || !e.path.chars().all(|c| {
                        c.is_ascii_digit() || "MmLlHhVvCcSsQqTtAaZz.,-+ eE\n\t".contains(c)
                    }))
            {
                return Err(bad());
            }
            if ["plot", "bars"].contains(&e.kind.as_str())
                && (e.points.len() < 2
                    || e.x_range[0] >= e.x_range[1]
                    || e.y_range[0] >= e.y_range[1]
                    || e.points.iter().any(|p| {
                        !p[0].is_finite()
                            || !p[1].is_finite()
                            || p[0] < e.x_range[0]
                            || p[0] > e.x_range[1]
                            || p[1] < e.y_range[0]
                            || p[1] > e.y_range[1]
                    }))
            {
                return Err(bad());
            }
            graphic |= !["text", "number", "rect"].contains(&e.kind.as_str());
        }
        let mut last = -1;
        let mut substantive = false;
        for event in &self.events {
            if !ids.contains(event.entity_id.as_str())
                || event.start_ms < last
                || event.start_ms < 0
                || event.duration_ms < 1
                || event.start_ms + event.duration_ms > duration_ms
                || event.reasoning.trim().is_empty()
                || ![
                    "move",
                    "flow",
                    "trace",
                    "draw",
                    "transform",
                    "split",
                    "merge",
                    "spawn",
                    "remove",
                    "grow",
                    "shrink",
                    "connect",
                    "disconnect",
                    "set_value",
                    "reveal",
                ]
                .contains(&event.action.as_str())
            {
                return Err(bad());
            }
            for n in [
                event.to.x,
                event.to.y,
                event.to.value,
                event.to.opacity,
                event.to.scale,
            ]
            .into_iter()
            .flatten()
            {
                if !n.is_finite() {
                    return Err(bad());
                }
            }
            if event.to.x.is_some_and(|v| !(0.0..=1.0).contains(&v))
                || event.to.y.is_some_and(|v| !(0.0..=1.0).contains(&v))
                || event.to.opacity.is_some_and(|v| !(0.0..=1.0).contains(&v))
                || event.to.scale.is_some_and(|v| !(0.1..=2.0).contains(&v))
                || event.to.color.as_ref().is_some_and(|c| !color(c))
            {
                return Err(bad());
            }
            let e = self
                .entities
                .iter()
                .find(|e| e.id == event.entity_id)
                .unwrap();
            if !["", "linear", "accelerate"].contains(&event.to.easing.as_str()) {
                return Err(bad());
            }
            if ["move", "flow", "split", "merge"].contains(&event.action.as_str())
                && event.to.x.is_none()
                && event.to.y.is_none()
                && event.to.opacity.is_none()
            {
                return Err(bad());
            }
            if event.action == "set_value"
                && !((e.kind == "number" && event.to.value.is_some())
                    || (e.kind == "text"
                        && event
                            .to
                            .text
                            .as_ref()
                            .is_some_and(|v| !v.is_empty() && v.chars().count() <= 60)))
            {
                return Err(ProviderError::invalid(format!(
                    "实体 {} 的 set_value 必须为 number 提供 to.value，或为 text 提供 to.text。",
                    e.id
                )));
            }
            if event.action == "transform" && event.to.scale.is_none() && event.to.color.is_none() {
                return Err(bad());
            }
            if ["trace", "draw"].contains(&event.action.as_str())
                && !["path", "vector", "plot", "bars"].contains(&e.kind.as_str())
            {
                return Err(ProviderError::invalid(
                    "draw/trace 只能作用于路径、向量或数据图。",
                ));
            }
            if ["grow", "shrink"].contains(&event.action.as_str())
                && !["plot", "bars"].contains(&e.kind.as_str())
                && event.to.scale.is_none()
            {
                return Err(ProviderError::invalid("grow/shrink 必须声明 to.scale。"));
            }
            if event.to.x.unwrap_or(e.x) + e.width > 1.001
                || event.to.y.unwrap_or(e.y) + e.height > 1.001
            {
                return Err(bad());
            }
            substantive |= !["text", "number", "rect"].contains(&e.kind.as_str())
                && !["spawn", "remove", "reveal"].contains(&event.action.as_str());
            last = event.start_ms;
        }
        if !graphic || !substantive {
            return Err(ProviderError::invalid(
                "场景只有文字卡片或显隐，请设计对象、曲线和有教学含义的动态过程。",
            ));
        }
        for e in &self.entities {
            if !e.sample_from.is_empty()
                && (e.kind != "number"
                    || !self
                        .entities
                        .iter()
                        .any(|p| p.id == e.sample_from && p.kind == "plot"))
            {
                return Err(ProviderError::invalid(
                    "sample_from 必须引用当前计划的 plot 对象。",
                ));
            }
        }
        for r in &self.relationships {
            if !ids.contains(r.from.as_str())
                || !ids.contains(r.to.as_str())
                || r.label.chars().count() > 40
            {
                return Err(bad());
            }
        }
        Ok(())
    }

    /// 同一计划重新布局：横屏左右叙事、竖屏上下叙事；所有坐标源自当前画布。
    pub fn svg(&self, duration_ms: i64, width: u32, height: u32) -> ProviderResult<String> {
        self.validate(duration_ms)?;
        let (w, h) = (width as f64, height as f64);
        let (bg, fg, accent, muted) =
            if self.style.contains("minimal") || self.style.contains("极简") {
                ("#F4F1E8", "#182E36", "#087F8C", "#536773")
            } else if self.style.contains("energetic") {
                ("#101424", "#FFF4E4", "#FF8B75", "#A5B1C6")
            } else {
                ("#0B1117", "#F1E8D5", "#D8B56E", "#9FAAAE")
            };
        let (bg, fg, accent, muted) = self
            .palette
            .as_ref()
            .map(|p| {
                (
                    p.background.as_str(),
                    p.foreground.as_str(),
                    p.accent.as_str(),
                    p.muted.as_str(),
                )
            })
            .unwrap_or((bg, fg, accent, muted));
        let readable_fg = readable_color(fg, bg, 4.5);
        let readable_muted = readable_color(muted, bg, 4.5);
        let readable_accent = readable_color(accent, bg, 3.);
        let (fg, muted, accent) = (
            readable_fg.as_str(),
            readable_muted.as_str(),
            readable_accent.as_str(),
        );
        let margin = w.min(h) * 0.06;
        let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><rect width=\"{width}\" height=\"{height}\" fill=\"{bg}\"/><defs><marker id=\"arrow\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"4\" orient=\"auto\"><path d=\"M0 0 L8 4 L0 8\" fill=\"{accent}\"/></marker></defs><g font-family=\"Arial,Hiragino Sans GB,Microsoft YaHei,Noto Sans CJK SC,sans-serif\">");
        svg += &label(
            margin,
            h * 0.075,
            14.,
            muted,
            "EXPLAINCRAFT · 科学图解",
            "start",
        );
        let title_size = (w * 0.88 / self.objective.chars().count().max(1) as f64).min(32.);
        svg += &label(margin, h * 0.145, title_size, fg, &self.objective, "start");
        let panels = if width > height {
            [
                (w * 0.06, h * 0.24, w * 0.46, h * 0.51),
                (w * 0.59, h * 0.24, w * 0.35, h * 0.51),
            ]
        } else if width == height {
            [
                (w * 0.06, h * 0.25, w * 0.41, h * 0.46),
                (w * 0.55, h * 0.25, w * 0.39, h * 0.46),
            ]
        } else {
            [
                (w * 0.10, h * 0.22, w * 0.80, h * 0.29),
                (w * 0.10, h * 0.59, w * 0.80, h * 0.22),
            ]
        };
        for (i, (x, y, pw, ph)) in panels.iter().enumerate() {
            svg += &format!(
                "<path d=\"M{x} {} H{}\" stroke=\"{accent}\" stroke-opacity=\"0.35\"/>",
                y - 20.,
                x + pw
            );
            svg += &label(
                *x,
                y - 32.,
                15.,
                muted,
                if i == 0 {
                    "01 / 观察对象"
                } else {
                    "02 / 数据与推理"
                },
                "start",
            );
            svg+=&format!("<clipPath id=\"panel{i}\"><rect x=\"{x}\" y=\"{}\" width=\"{pw}\" height=\"{}\"/></clipPath>",y-4.,ph+38.);
        }
        let box_of = |e: &Entity| {
            let (x, y, pw, ph) = panels[usize::from(e.region == "secondary")];
            (
                x + e.x * pw,
                y + e.y * ph,
                e.width * pw,
                e.height * ph,
                pw,
                ph,
            )
        };
        for r in &self.relationships {
            let a = self.entities.iter().find(|e| e.id == r.from).unwrap();
            let b = self.entities.iter().find(|e| e.id == r.to).unwrap();
            let (ax, ay, aw, ah, _, _) = box_of(a);
            let (bx, by, bw, bh, _, _) = box_of(b);
            svg+=&format!("<path d=\"M{} {} L{} {}\" fill=\"none\" stroke=\"{accent}\" stroke-opacity=\"0.4\" marker-end=\"url(#arrow)\"/>",ax+aw/2.,ay+ah/2.,bx+bw/2.,by+bh/2.);
        }
        for e in &self.entities {
            let (x, y, ew, eh, pw, ph) = box_of(e);
            let ink = if color(&e.color) {
                e.color.as_str()
            } else {
                accent
            };
            let events: Vec<_> = self.events.iter().filter(|v| v.entity_id == e.id).collect();
            let hidden = events
                .first()
                .is_some_and(|v| ["spawn", "reveal", "connect"].contains(&v.action.as_str()));
            svg += &format!(
                "<g id=\"{}\" clip-path=\"url(#panel{})\"><g color=\"{ink}\" opacity=\"{}\">",
                xml(&e.id),
                usize::from(e.region == "secondary"),
                if hidden { 0 } else { 1 }
            );
            let (mut px, mut py, mut opacity) = (e.x, e.y, if hidden { 0. } else { 1. });
            for event in &events {
                let a = event.start_ms;
                let d = event.duration_ms;
                if event.to.x.is_some() || event.to.y.is_some() {
                    let (nx, ny) = (event.to.x.unwrap_or(px), event.to.y.unwrap_or(py));
                    let values = (0..=8)
                        .map(|i| {
                            let f = i as f64 / 8.;
                            let f = if event.to.easing == "accelerate" {
                                f * f
                            } else {
                                f
                            };
                            format!(
                                "{} {}",
                                (px - e.x + (nx - px) * f) * pw,
                                (py - e.y + (ny - py) * f) * ph
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    svg+=&format!("<animateTransform attributeName=\"transform\" type=\"translate\" values=\"{values}\" begin=\"{a}ms\" dur=\"{d}ms\" fill=\"freeze\"/>");
                    px = nx;
                    py = ny;
                }
                let next = event.to.opacity.or_else(|| match event.action.as_str() {
                    "remove" | "disconnect" => Some(0.),
                    "spawn" | "reveal" | "connect" => Some(1.),
                    _ => None,
                });
                if let Some(next) = next {
                    svg += &anim("opacity", &opacity.to_string(), &next.to_string(), a, d);
                    opacity = next;
                }
            }
            let mut previous_color = ink.to_string();
            for ev in &events {
                if let Some(next) = &ev.to.color {
                    svg += &anim("color", &previous_color, next, ev.start_ms, ev.duration_ms);
                    previous_color = next.clone();
                }
            }
            // 缩放围绕对象中心；与移动分层，两个 transform 不互相覆盖。
            let (cx, cy) = (x + ew / 2., y + eh / 2.);
            svg += &format!("<g transform=\"translate({cx} {cy})\"><g transform=\"scale(1)\">");
            let mut scale = 1.;
            for ev in &events {
                if let Some(next) = ev.to.scale {
                    svg+=&format!("<animateTransform attributeName=\"transform\" type=\"scale\" from=\"{scale}\" to=\"{next}\" begin=\"{}ms\" dur=\"{}ms\" fill=\"freeze\"/>",ev.start_ms,ev.duration_ms);
                    scale = next;
                }
            }
            svg += &format!("<g transform=\"translate({} {})\">", -cx, -cy);
            match e.kind.as_str() {
                "path" => {
                    let draw=events.iter().find(|v|["draw","trace"].contains(&v.action.as_str()));
                    let length=path_length(&e.path).ok_or_else(||ProviderError::invalid("对象路径不能解析。"))?;
                    let offset=if draw.is_some(){length}else{0.};
                    let animation=draw.map(|v|anim("stroke-dashoffset",&length.to_string(),"0",v.start_ms,v.duration_ms)).unwrap_or_default();
                    svg+=&format!("<path d=\"{}\" transform=\"translate({x} {y}) scale({} {})\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"{}\" stroke-dasharray=\"{length}\" stroke-dashoffset=\"{offset}\">{animation}</path>",xml(&e.path),ew/100.,eh/100.,2./(ew/100.).max(eh/100.));
                },
                "ellipse"=>svg+=&format!("<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\"/>",x+ew/2.,y+eh/2.,ew/2.,eh/2.),
                "rect"=>svg+=&format!("<rect x=\"{x}\" y=\"{y}\" width=\"{ew}\" height=\"{eh}\" fill=\"currentColor\" fill-opacity=\"0.12\" stroke=\"currentColor\" stroke-width=\"2\"/>"),
                "vector"=>{
                    let length=ew;
                    let draw=events.iter().find(|v|["draw","trace"].contains(&v.action.as_str()));
                    let animation=draw.map(|v|anim("stroke-dashoffset",&length.to_string(),"0",v.start_ms,v.duration_ms)).unwrap_or_default();
                    let offset=if draw.is_some(){length}else{0.};
                    svg+=&format!("<path d=\"M{x} {} H{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"3\" stroke-dasharray=\"{length}\" stroke-dashoffset=\"{offset}\" marker-end=\"url(#arrow)\">{animation}</path>",y+eh/2.,x+ew);
                },
                "particles"=>{
                    // 每个粒子由固定种子派生轨迹，不使用逐帧随机数，也不冒充统计实验数据。
                    for i in 0..e.count.max(1){
                        let mut n=i.wrapping_mul(747796405).wrapping_add(e.seed).wrapping_add(2891336453);
                        n=((n>>((n>>28)+4))^n).wrapping_mul(277803737);n=(n>>22)^n;
                        let phase=(n%10007)as f64/10007.*std::f64::consts::TAU;
                        let phase_y=((n/10007)%10009)as f64/10009.*std::f64::consts::TAU;
                        let xs=(0..=16).map(|k|format!("{}",x+ew*(0.5+0.42*(phase+k as f64/16.*std::f64::consts::TAU*0.8).sin()))).collect::<Vec<_>>();
                        let ys=(0..=16).map(|k|format!("{}",y+eh*(0.5+0.41*(phase_y+k as f64/16.*std::f64::consts::TAU*0.65).cos()))).collect::<Vec<_>>();
                        svg+=&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"currentColor\" opacity=\"0.8\"><animate attributeName=\"cx\" values=\"{}\" dur=\"{duration_ms}ms\" fill=\"freeze\"/><animate attributeName=\"cy\" values=\"{}\" dur=\"{duration_ms}ms\" fill=\"freeze\"/></circle>",xs[0],ys[0],(ew.min(eh)*0.022).clamp(2.,6.),xs.join(";"),ys.join(";"));
                    }
                }
                "plot"|"bars"=>{
                    let left=x+ew*0.14;let top=y+eh*0.08;let cw=ew*0.82;let ch=eh*0.74;
                    svg+=&format!("<path d=\"M{left} {top} V{} H{}\" fill=\"none\" stroke=\"{muted}\" stroke-width=\"1.5\"/>",top+ch,left+cw);
                    for tick in 0..=4{let f=tick as f64/4.;let ty=top+ch*(1.-f);svg+=&format!("<path d=\"M{left} {ty} H{}\" stroke=\"{muted}\" stroke-opacity=\"0.15\"/>",left+cw);svg+=&label(left-8.,ty+5.,14.,muted,&format!("{:.2}",e.y_range[0]+f*(e.y_range[1]-e.y_range[0])),"end");}
                    for tick in 0..=4 {let f=tick as f64/4.;svg+=&label(left+cw*f,top+ch+20.,13.,muted,&format!("{:.0}",e.x_range[0]+f*(e.x_range[1]-e.x_range[0])),"middle");}
                    svg+=&label(left+cw/2.,top+ch+45.,16.,muted,&e.x_label,"middle");svg+=&label(left,top-8.,16.,muted,&e.y_label,"start");
                    let pts:Vec<_>=e.points.iter().map(|p|(left+(p[0]-e.x_range[0])/(e.x_range[1]-e.x_range[0])*cw,top+ch-(p[1]-e.y_range[0])/(e.y_range[1]-e.y_range[0])*ch)).collect();
                    let event=events.iter().find(|v|["draw","trace","grow"].contains(&v.action.as_str()));let (a,d)=event.map(|v|(v.start_ms,v.duration_ms)).unwrap_or((0,duration_ms));
                    if e.kind=="plot"{let length=pts.windows(2).map(|p|((p[1].0-p[0].0).powi(2)+(p[1].1-p[0].1).powi(2)).sqrt()).sum::<f64>();let points=pts.iter().map(|(x,y)|format!("{x},{y}")).collect::<Vec<_>>().join(" ");
                        svg+=&format!("<polyline points=\"{points}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"3\" stroke-linejoin=\"round\" stroke-dasharray=\"{length}\" stroke-dashoffset=\"{length}\">{}</polyline>",{
                            let mut distance=0.;let mut offsets=vec![length.to_string()];let mut times=vec!["0".to_string()];
                            for(i,p)in pts.iter().enumerate(){if i>0 {distance+=((p.0-pts[i-1].0).powi(2)+(p.1-pts[i-1].1).powi(2)).sqrt();}let t=(e.points[i][0]-e.x_range[0])/(e.x_range[1]-e.x_range[0]);if t>0. {offsets.push((length-distance).max(0.).to_string());times.push(t.to_string());}}
                            if times.last().is_none_or(|t|t!="1"){offsets.push("0".into());times.push("1".into());}
                            format!("<animate attributeName=\"stroke-dashoffset\" values=\"{}\" keyTimes=\"{}\" begin=\"{a}ms\" dur=\"{d}ms\" fill=\"freeze\"/>",offsets.join(";"),times.join(";"))
                        });
                    } else {for (i,(bx,by)) in pts.iter().enumerate(){let bh=top+ch-by;svg+=&format!("<rect x=\"{bx}\" y=\"{}\" width=\"{}\" height=\"0\" fill=\"currentColor\">{}{}</rect>",top+ch,cw/pts.len()as f64*0.5,anim("height","0",&bh.to_string(),a+i as i64*80,d),anim("y",&(top+ch).to_string(),&by.to_string(),a+i as i64*80,d));}}
                }
                "number"=>{
                    let mut values=vec![(0,e.value)];
                    if let Some(plot)=self.entities.iter().find(|p|p.id==e.sample_from && p.kind=="plot") {
                        let trace=self.events.iter().find(|ev|ev.entity_id==plot.id && ["draw","trace","grow"].contains(&ev.action.as_str()));
                        let (start,span)=trace.map(|e|(e.start_ms,e.duration_ms)).unwrap_or((0,duration_ms));
                        values=vec![(0,plot.points[0][1])];
                        for p in &plot.points {values.push((start+((p[0]-plot.x_range[0])/(plot.x_range[1]-plot.x_range[0])*span as f64)as i64,p[1]));}
                    } else {for ev in &events {if let Some(v)=ev.to.value {values.push((ev.start_ms+ev.duration_ms,v));}}}
                    values.sort_by_key(|p|p.0);values.dedup_by_key(|p|p.0);
                    for (i,(at,value)) in values.iter().enumerate(){let end=values.get(i+1).map(|v|v.0).unwrap_or(duration_ms);svg+=&format!("<g opacity=\"0\"><set attributeName=\"opacity\" to=\"1\" begin=\"{at}ms\"/>{}<set attributeName=\"opacity\" to=\"0\" begin=\"{end}ms\"/></g>",label(x,y+eh*0.65,(ew/(format!("{value:.2} {}",e.unit).chars().map(|c|if c.is_ascii(){0.56}else{1.}).sum::<f64>().max(1.))).min(48.),ink,&format!("{value:.2} {}",e.unit),"start"));}
                }
                "text"=>{
                    let mut states=vec![(0,e.label.clone())];for ev in &events{if let Some(text)=&ev.to.text{states.push((ev.start_ms+ev.duration_ms,text.clone()));}}
                    states.sort_by_key(|s|s.0);
                    for(i,(at,value))in states.iter().enumerate(){let end=states.get(i+1).map(|s|s.0).unwrap_or(duration_ms);let size=(ew/value.chars().map(|c|if c.is_ascii(){0.56}else{1.}).sum::<f64>().max(1.)).min(24.);svg+=&format!("<g opacity=\"0\"><set attributeName=\"opacity\" to=\"1\" begin=\"{at}ms\"/>{}<set attributeName=\"opacity\" to=\"0\" begin=\"{end}ms\"/></g>",label(x,y+eh*0.65,size,fg,value,"start"));}
                },
                _=>{}
            }
            if !e.label.is_empty() && !["text", "number", "plot", "bars"].contains(&e.kind.as_str())
            {
                svg += &label(x + ew / 2., y + eh + 25., 19., fg, &e.label, "middle");
            }
            svg += "</g></g></g></g></g>";
        }
        let conclusion_size = (w * 0.88 / self.final_state.chars().count().max(1) as f64).min(24.);
        svg += &label(
            w * 0.5,
            h * if width >= height { 0.76 } else { 0.85 },
            conclusion_size,
            fg,
            &self.final_state,
            "middle",
        );
        svg += "</g></svg>";
        Ok(svg)
    }
}
fn label(x: f64, y: f64, size: f64, color: &str, text: &str, anchor: &str) -> String {
    format!("<text x=\"{x}\" y=\"{y}\" font-size=\"{size}\" fill=\"{color}\" text-anchor=\"{anchor}\">{}</text>",xml(text))
}
fn anim(name: &str, from: &str, to: &str, start: i64, duration: i64) -> String {
    format!("<animate attributeName=\"{name}\" from=\"{from}\" to=\"{to}\" begin=\"{start}ms\" dur=\"{duration}ms\" fill=\"freeze\"/>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::*;
    fn fixtures() -> Vec<(&'static str, ScenePlan)> {
        [
            (
                "probability",
                include_str!("../tests/fixtures/scenes/probability.json"),
            ),
            ("tcp", include_str!("../tests/fixtures/scenes/tcp.json")),
            (
                "newton",
                include_str!("../tests/fixtures/scenes/newton.json"),
            ),
        ]
        .into_iter()
        .map(|(n, s)| (n, serde_json::from_str(s).unwrap()))
        .collect()
    }
    #[test]
    fn scene_protocol_rejects_text_only_unknown_ids_and_unbounded_events() {
        for (_, plan) in fixtures() {
            plan.validate(6000).unwrap();
            let mut bad = plan.clone();
            bad.events[0].entity_id = "missing".into();
            assert!(bad.validate(6000).is_err());
            let mut bad = plan.clone();
            bad.events[0].duration_ms = 7000;
            assert!(bad.validate(6000).is_err());
        }
        let mut p = fixtures().remove(0).1;
        for e in &mut p.entities {
            e.kind = "text".into();
        }
        assert!(p.validate(6000).is_err());
    }
    #[test]
    fn all_aspects_have_native_canvas_and_reflow_panels() {
        for (_, mut plan) in fixtures() {
            for ratio in ["16:9", "9:16", "1:1"] {
                let (w, h) = crate::util::dimensions(ratio);
                plan.aspect_ratio = ratio.into();
                let svg = plan.svg(6000, w as u32, h as u32).unwrap();
                crate::svg_animation::validate_canvas(&svg, w as u32, h as u32).unwrap();
                assert!(svg.contains(&format!("viewBox=\"0 0 {w} {h}\"")));
            }
        }
    }
    /// 可复现数字由测试独立计算，不能用动画看起来合理代替数学和协议核验。
    #[test]
    fn fixture_numbers_are_computed_and_not_invented() {
        let p = fixtures().remove(0).1;
        let curve = p.entities.iter().find(|e| e.id == "curve").unwrap();
        let (mut seed, mut heads) = (20261115u32, 0.);
        for (i, point) in curve.points.iter().enumerate() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            heads += ((seed >> 24) & 1) as f64;
            assert!((point[1] - heads / (i + 1) as f64).abs() < 1e-12);
        }
        let n = fixtures().remove(2).1;
        let v = n.entities.iter().find(|e| e.id == "velocity").unwrap();
        for p in &v.points {
            assert_eq!(p[1], 12. / 3. * p[0]);
        }
        assert_eq!(0.5 * 4. * 4f64.powi(2), 32.);
        let tcp = fixtures().remove(1).1;
        let messages: Vec<_> = tcp
            .events
            .iter()
            .filter(|e| e.action == "flow")
            .map(|e| e.entity_id.as_str())
            .collect();
        assert_eq!(messages, vec!["syn", "synack", "ack"]);
        let arrival = |id: &str| {
            let e = tcp
                .events
                .iter()
                .find(|e| e.entity_id == id && e.action == "flow")
                .unwrap();
            e.start_ms + e.duration_ms
        };
        let reveal = |id: &str| {
            tcp.events
                .iter()
                .find(|e| e.entity_id == id && e.action == "reveal")
                .unwrap()
                .start_ms
        };
        assert_eq!(
            reveal("client_ready"),
            arrival("synack"),
            "RFC 9293: client establishes on SYN+ACK arrival"
        );
        assert_eq!(
            reveal("established"),
            arrival("ack"),
            "server establishes after final ACK"
        );
    }
    /// 六个真实 MP4 加方形/1080p 样例，全部复用同一协议与编译器，不访问模型。
    #[tokio::test]
    async fn render_cross_discipline_native_videos() {
        let root = std::env::var("EXPLAINCRAFT_ARTIFACT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join(crate::util::make_id("scene-evidence")));
        std::fs::create_dir_all(&root).unwrap();
        for (name, mut plan) in fixtures() {
            for ratio in ["16:9", "9:16", "1:1"] {
                plan.aspect_ratio = ratio.into();
                let (w, h) = crate::util::dimensions(ratio);
                let svg = plan.svg(6000, w as u32, h as u32).unwrap();
                let stem = format!("{name}-{}", ratio.replace(':', "x"));
                std::fs::write(root.join(format!("{stem}.svg")), &svg).unwrap();
                let rendered = crate::svg_animation::render(&svg, 6000, w as u32, h as u32, 0)
                    .await
                    .unwrap();
                let MediaData::Bytes(bytes) = rendered.data else {
                    panic!("expected video")
                };
                let raw = root.join(format!("{stem}-raw.mp4"));
                std::fs::write(&raw, &bytes).unwrap();
                // 合成沿用产品的直接输出器，字幕与一段本地确定性音轨共享同一时间线。
                let voice = root.join(format!("{name}-voice.wav"));
                if !voice.exists() {
                    let status = std::process::Command::new("ffmpeg")
                        .args([
                            "-v",
                            "error",
                            "-y",
                            "-f",
                            "lavfi",
                            "-i",
                            "sine=frequency=440:sample_rate=48000:duration=6",
                        ])
                        .arg(&voice)
                        .status()
                        .unwrap();
                    assert!(status.success());
                }
                let mut draft = crate::catalog::empty_draft();
                draft.settings.aspect_ratio = ratio.into();
                let ctx = ProductionContext {
                    brief: None,
                    settings: draft.settings,
                    user_text: "离线协议验收，音轨为测试信号，不冒充配音".into(),
                    answers: vec![],
                    speech_rate: 1.,
                };
                let cfg = crate::Config::from_env();
                let renderer = crate::providers::direct::DirectRenderer::new(&cfg);
                let narration = match name {
                    "probability" => "固定样本中，累计频率会波动。一次实验不能保证每次都接近概率。",
                    "tcp" => "客户端发送SYN，服务器回复SYN加ACK，客户端确认ACK，双方建立连接。",
                    _ => "合外力十二牛，质量三千克，加速度为四米每二次方秒。",
                };
                let timeline = EditPlan {
                    aspect_ratio: ratio.into(),
                    output_resolution: "720p".into(),
                    total_ms: 6000,
                    clips: vec![Clip {
                        shot_id: name.into(),
                        duration_ms: 6000,
                        visual_asset_id: Some("visual".into()),
                        narration_asset_id: Some("voice".into()),
                        subtitle: Some(narration.into()),
                        ..Default::default()
                    }],
                    narration_volume: 0.4,
                    subtitles_enabled: true,
                    ..Default::default()
                };
                let output = renderer
                    .render(RenderRequest {
                        ctx,
                        plan: timeline,
                        inputs: std::collections::HashMap::from([
                            ("visual".into(), raw.clone()),
                            ("voice".into(), voice.clone()),
                        ]),
                    })
                    .await
                    .unwrap();
                let path = root.join(format!("{stem}.mp4"));
                match output.data {
                    MediaData::Bytes(bytes) => std::fs::write(&path, bytes).unwrap(),
                    MediaData::Path(source) => {
                        std::fs::copy(source, &path).unwrap();
                    }
                }
                crate::quality::inspect(&path, 6000, w, h, true)
                    .await
                    .unwrap();
                let audit = crate::quality::visual_audit(&path, 6000, Some(&plan)).await;
                assert_ne!(audit["suspected_pillarbox"], true);
                std::fs::write(
                    root.join(format!("{stem}-audit.json")),
                    serde_json::to_vec_pretty(&audit).unwrap(),
                )
                .unwrap();
                for at in [0.5, 2.5, 4.8] {
                    let target = root.join(format!("{stem}-{at:.1}s.png"));
                    let status = std::process::Command::new("ffmpeg")
                        .args(["-v", "error", "-y", "-ss", &at.to_string(), "-i"])
                        .arg(&path)
                        .args(["-frames:v", "1"])
                        .arg(&target)
                        .status()
                        .unwrap();
                    assert!(status.success());
                }
            }
        }
        let plan = fixtures().remove(0).1;
        let svg = plan.svg(1000.max(6000), 1920, 1080).unwrap();
        let video = crate::svg_animation::render(&svg, 6000, 1920, 1080, 0)
            .await
            .unwrap();
        let MediaData::Bytes(bytes) = video.data else {
            panic!("video")
        };
        let path = root.join("probability-16x9-1080p.mp4");
        std::fs::write(&path, bytes).unwrap();
        crate::quality::inspect(&path, 6000, 1920, 1080, false)
            .await
            .unwrap();
        eprintln!("Evidence: {}", root.display());
    }
}

/// 结构化输出与 Rust 协议共用字段，所有可省值显式写默认值或 null，避免供应商隐式扩展字段。
pub fn json_schema(duration_ms: i64) -> serde_json::Value {
    use serde_json::{json, Value};
    fn object(properties: Value) -> Value {
        let required: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
        json!({"type":"object","additionalProperties":false,"required":required,"properties":properties})
    }
    let number = json!({"type":"number"});
    let text = json!({"type":"string"});
    let unit = json!({"type":"number","minimum":0,"maximum":1});
    let nullable_number = json!({"type":["number","null"]});
    let pair = json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2});
    let entity = object(
        json!({"id":text,"kind":{"type":"string","enum":["path","ellipse","particles","plot","bars","vector","number","text","rect"]},"label":text,"region":{"type":"string","enum":["primary","secondary"]},"x":unit,"y":unit,"width":unit,"height":unit,"path":text,"points":{"type":"array","items":pair},"count":{"type":"integer","minimum":0,"maximum":160},"seed":{"type":"integer","minimum":0,"maximum":4294967295u64},"value":number,"unit":text,"color":text,"x_range":pair,"y_range":pair,"x_label":text,"y_label":text,"sample_from":text}),
    );
    let change = object(
        json!({"x":nullable_number,"y":nullable_number,"value":nullable_number,"text":{"type":["string","null"]},"opacity":nullable_number,"scale":nullable_number,"color":{"type":["string","null"]},"easing":{"type":"string","enum":["","linear","accelerate"]}}),
    );
    let event = object(
        json!({"entity_id":text,"action":{"type":"string","enum":["move","flow","trace","draw","transform","split","merge","spawn","remove","grow","shrink","connect","disconnect","set_value","reveal"]},"start_ms":{"type":"integer","minimum":0,"maximum":duration_ms},"duration_ms":{"type":"integer","minimum":1,"maximum":duration_ms},"to":change,"reasoning":text}),
    );
    object(
        json!({"objective":text,"visual_concept":text,"entities":{"type":"array","minItems":1,"maxItems":48,"items":entity},"relationships":{"type":"array","items":object(json!({"from":text,"to":text,"label":text}))},"events":{"type":"array","minItems":1,"maxItems":120,"items":event},"final_state":text,"style":text,"aspect_ratio":{"type":"string","enum":["16:9","9:16","1:1"]},"claims":{"type":"array","items":object(json!({"statement":text,"evidence":text}))},"palette":{"anyOf":[{"type":"null"},object(json!({"background":text,"foreground":text,"accent":text,"muted":text}))]}}),
    )
}

/// 保留模型配色方向并提高可读性；小标签不能靠低对比度制造纪录片氛围。
fn readable_color(foreground: &str, background: &str, minimum: f64) -> String {
    fn rgb(s: &str) -> [f64; 3] {
        [
            u8::from_str_radix(&s[1..3], 16).unwrap_or(0) as f64,
            u8::from_str_radix(&s[3..5], 16).unwrap_or(0) as f64,
            u8::from_str_radix(&s[5..7], 16).unwrap_or(0) as f64,
        ]
    }
    fn luma(c: [f64; 3]) -> f64 {
        let v = c.map(|v| {
            let s = v / 255.;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        });
        v[0] * 0.2126 + v[1] * 0.7152 + v[2] * 0.0722
    }
    let source = rgb(foreground);
    let bg = luma(rgb(background));
    let target = if bg > 0.18 { 0. } else { 255. };
    for i in 0..=20 {
        let p = i as f64 / 20.;
        let c = source.map(|v| v + (target - v) * p);
        let lum = luma(c);
        if (bg.max(lum) + 0.05) / (bg.min(lum) + 0.05) >= minimum {
            return format!(
                "#{:02X}{:02X}{:02X}",
                c[0].round() as u8,
                c[1].round() as u8,
                c[2].round() as u8
            );
        }
    }
    if bg > 0.18 {
        "#000000".into()
    } else {
        "#FFFFFF".into()
    }
}

#[cfg(test)]
mod evidence_replay {
    use super::*;
    use crate::providers::*;
    /// 只回放已经保存的代码与计划，最后整理画面时不会再次调用任何 API。
    #[tokio::test]
    #[ignore = "offline evidence replay; requires saved artifacts"]
    async fn replay_saved_scene_evidence() {
        let base = std::path::PathBuf::from(std::env::var("EXPLAINCRAFT_ARTIFACT_DIR").unwrap());
        let root = base.join("paid-bounded");
        for name in ["probability", "newton", "tcp"] {
            for ratio in ["16:9", "9:16"] {
                let stem = format!("{name}-{}", ratio.replace(':', "x"));
                let plan: ScenePlan = serde_json::from_slice(
                    &std::fs::read(root.join(format!("{stem}-plan.json"))).unwrap(),
                )
                .unwrap();
                let (w, h) = crate::util::dimensions(ratio);
                let svg = plan.svg(6000, w as u32, h as u32).unwrap();
                std::fs::write(root.join(format!("{stem}.svg")), &svg).unwrap();
                let raw = root.join(format!("{stem}-raw.mp4"));
                let rendered = crate::svg_animation::render(&svg, 6000, w as u32, h as u32, 0)
                    .await
                    .unwrap();
                match rendered.data {
                    MediaData::Bytes(b) => std::fs::write(&raw, b).unwrap(),
                    MediaData::Path(p) => {
                        std::fs::copy(p, &raw).unwrap();
                    }
                }
                let mut draft = crate::catalog::empty_draft();
                draft.settings.aspect_ratio = ratio.into();
                let ctx = ProductionContext {
                    brief: None,
                    settings: draft.settings,
                    user_text: "已保存导演计划的离线回放，无模型调用".into(),
                    answers: vec![],
                    speech_rate: 1.,
                };
                let timeline = EditPlan {
                    aspect_ratio: ratio.into(),
                    output_resolution: "720p".into(),
                    total_ms: 6000,
                    clips: vec![Clip {
                        shot_id: name.into(),
                        duration_ms: 6000,
                        visual_asset_id: Some("v".into()),
                        narration_asset_id: Some("a".into()),
                        subtitle: Some(
                            std::fs::read_to_string(base.join(format!("{name}-narration.txt")))
                                .unwrap(),
                        ),
                        ..Default::default()
                    }],
                    narration_volume: 1.,
                    subtitles_enabled: true,
                    ..Default::default()
                };
                let output = direct::DirectRenderer::new(&crate::Config::from_env())
                    .render(RenderRequest {
                        ctx,
                        plan: timeline,
                        inputs: std::collections::HashMap::from([
                            ("v".into(), raw),
                            ("a".into(), base.join(format!("{name}-voice.wav"))),
                        ]),
                    })
                    .await
                    .unwrap();
                let path = root.join(format!("{stem}.mp4"));
                match output.data {
                    MediaData::Bytes(b) => std::fs::write(&path, b).unwrap(),
                    MediaData::Path(p) => {
                        std::fs::copy(p, &path).unwrap();
                    }
                }
                crate::quality::inspect(&path, 6000, w, h, true)
                    .await
                    .unwrap();
                for at in [0.5, 2.5, 5.5] {
                    assert!(std::process::Command::new("ffmpeg")
                        .args(["-v", "error", "-y", "-ss", &at.to_string(), "-i"])
                        .arg(&path)
                        .args(["-frames:v", "1"])
                        .arg(root.join(format!("{stem}-{at:.1}s.png")))
                        .status()
                        .unwrap()
                        .success());
                }
            }
        }
    }
    /// SVG 与 HTML 使用同一计划，在真实离线 Chrome 中验证横竖原生画幅。
    #[tokio::test]
    async fn native_html_scene_layouts() {
        let mut plan: ScenePlan =
            serde_json::from_str(include_str!("../tests/fixtures/scenes/probability.json"))
                .unwrap();
        for e in &mut plan.events {
            e.start_ms /= 2;
            e.duration_ms = (e.duration_ms / 2).max(1);
        }
        let root = std::env::var("EXPLAINCRAFT_ARTIFACT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join(crate::util::make_id("native-html")));
        std::fs::create_dir_all(&root).unwrap();
        for ratio in ["16:9", "9:16"] {
            plan.aspect_ratio = ratio.into();
            let (w, h) = crate::util::dimensions(ratio);
            let svg = plan.svg(3000, w as u32, h as u32).unwrap();
            let video = crate::html_animation::render(&svg, 3000, w as u32, h as u32, 0)
                .await
                .unwrap();
            let MediaData::Bytes(b) = video.data else {
                panic!("video")
            };
            let path = root.join(format!("html-{}.mp4", ratio.replace(':', "x")));
            std::fs::write(&path, b).unwrap();
            crate::quality::inspect(&path, 3000, w, h, false)
                .await
                .unwrap();
        }
    }
}

/// 以有限采样估计复合路径长度，供逐笔绘制使用；不把动画时间绑定到不可信脚本。
fn path_length(d: &str) -> Option<f64> {
    use tiny_skia::PathSegment;
    let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\" viewBox=\"0 0 100 100\"><path d=\"{}\" fill=\"none\" stroke=\"black\"/></svg>",xml(d));
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let path = tree.root().children().iter().find_map(|n| {
        if let usvg::Node::Path(p) = n {
            Some(p)
        } else {
            None
        }
    })?;
    let (mut current, mut start, mut length) = ((0., 0.), (0., 0.), 0.);
    let point = |p: tiny_skia::Point| (p.x as f64, p.y as f64);
    let dist = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    for segment in path.data().segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                current = point(p);
                start = current;
            }
            PathSegment::LineTo(p) => {
                let p = point(p);
                length += dist(current, p);
                current = p;
            }
            PathSegment::QuadTo(c, p) => {
                let a = current;
                let b = point(c);
                let end = point(p);
                for i in 1..=16 {
                    let t = i as f64 / 16.;
                    let q = (
                        (1. - t).powi(2) * a.0 + 2. * (1. - t) * t * b.0 + t * t * end.0,
                        (1. - t).powi(2) * a.1 + 2. * (1. - t) * t * b.1 + t * t * end.1,
                    );
                    length += dist(current, q);
                    current = q;
                }
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let a = current;
                let b = point(c1);
                let c = point(c2);
                let end = point(p);
                for i in 1..=16 {
                    let t = i as f64 / 16.;
                    let u = 1. - t;
                    let q = (
                        u.powi(3) * a.0
                            + 3. * u * u * t * b.0
                            + 3. * u * t * t * c.0
                            + t.powi(3) * end.0,
                        u.powi(3) * a.1
                            + 3. * u * u * t * b.1
                            + 3. * u * t * t * c.1
                            + t.powi(3) * end.1,
                    );
                    length += dist(current, q);
                    current = q;
                }
            }
            PathSegment::Close => {
                length += dist(current, start);
                current = start;
            }
        }
    }
    (length.is_finite() && length > 0.).then_some(length)
}
