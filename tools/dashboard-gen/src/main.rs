//! Dashboard generator — reads `metrics.toml` files from backend crates and produces
//! Grafana dashboard JSON and Prometheus alert rule YAML.
//!
//! Usage:
//! ```sh
//! dashboard-gen \
//!     --input ../../backend/crates \
//!     --dashboards-output ../../infra/grafana/dashboards/generated \
//!     --alerts-output ../../infra/prometheus/alerts/generated
//! ```

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

/// Generate Grafana dashboards and Prometheus alert rules from metrics.toml files.
#[derive(Parser, Debug)]
#[command(name = "dashboard-gen", version)]
struct Cli {
    /// Path to the directory containing backend crates (e.g., backend/crates).
    #[arg(long)]
    input: PathBuf,

    /// Output directory for generated Grafana dashboard JSON files.
    #[arg(long)]
    dashboards_output: PathBuf,

    /// Output directory for generated Prometheus alert rule YAML files.
    #[arg(long)]
    alerts_output: PathBuf,
}

// ---------------------------------------------------------------------------
// metrics.toml model
// ---------------------------------------------------------------------------

/// Flat format: `[crate]` + `[[metrics]]` (sober-api, sober-core, sober-db, etc.)
#[derive(Debug, Deserialize)]
struct FlatMetricsFile {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
    #[serde(default)]
    metrics: Vec<MetricDef>,
}

/// Grouped format: `[group.<name>]` + `[[group.<name>.metrics]]` (sober-agent, sober-scheduler)
#[derive(Debug, Deserialize)]
struct GroupedMetricsFile {
    #[serde(default)]
    group: BTreeMap<String, GroupDef>,
}

#[derive(Debug, Deserialize)]
struct GroupDef {
    name: String,
    #[allow(dead_code)]
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    metrics: Vec<GroupedMetricDef>,
}

/// A metric in the grouped format uses `description` instead of `help`.
#[derive(Debug, Deserialize, Clone)]
struct GroupedMetricDef {
    name: String,
    #[serde(rename = "type")]
    metric_type: String,
    description: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    buckets: Option<Vec<f64>>,
}

/// Normalised representation used by both formats.
#[derive(Debug, Deserialize)]
struct MetricsFile {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
    #[serde(default)]
    metrics: Vec<MetricDef>,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    name: String,
    dashboard_title: String,
}

#[derive(Debug, Deserialize, Clone)]
struct MetricDef {
    name: String,
    #[serde(rename = "type")]
    metric_type: String,
    help: String,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    buckets: Option<Vec<f64>>,
    #[serde(default)]
    alerts: Vec<AlertDef>,
}

#[derive(Debug, Deserialize, Clone)]
struct AlertDef {
    name: String,
    severity: String,
    expr: String,
    #[serde(rename = "for")]
    for_duration: String,
    summary: String,
}

/// Parse a metrics.toml that may be in either flat or grouped format.
fn parse_metrics_file(content: &str, path: &std::path::Path) -> Result<MetricsFile> {
    // Try flat format first (has [crate] section)
    if let Ok(flat) = toml::from_str::<FlatMetricsFile>(content) {
        return Ok(MetricsFile {
            crate_info: flat.crate_info,
            metrics: flat.metrics,
        });
    }

    // Try grouped format (has [group.*] sections)
    let grouped: GroupedMetricsFile = toml::from_str(content)
        .with_context(|| format!("Failed to parse {} as either flat or grouped format", path.display()))?;

    // Derive crate name from directory name
    let crate_name = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let dashboard_title = crate_name
        .strip_prefix("sober-")
        .unwrap_or(&crate_name)
        .to_string();
    let dashboard_title = format!(
        "{}{}",
        dashboard_title[..1].to_uppercase(),
        &dashboard_title[1..]
    );

    let mut metrics = Vec::new();
    for (_key, group) in &grouped.group {
        for gm in &group.metrics {
            metrics.push(MetricDef {
                name: gm.name.clone(),
                metric_type: gm.metric_type.clone(),
                help: gm.description.clone(),
                labels: gm.labels.clone(),
                group: Some(group.name.clone()),
                buckets: gm.buckets.clone(),
                alerts: Vec::new(),
            });
        }
    }

    Ok(MetricsFile {
        crate_info: CrateInfo {
            name: crate_name,
            dashboard_title,
        },
        metrics,
    })
}

// ---------------------------------------------------------------------------
// Prometheus alert rule model (for YAML output)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct AlertRuleFile {
    groups: Vec<AlertGroup>,
}

#[derive(Debug, Serialize)]
struct AlertGroup {
    name: String,
    rules: Vec<AlertRule>,
}

#[derive(Debug, Serialize)]
struct AlertRule {
    alert: String,
    expr: String,
    #[serde(rename = "for")]
    for_duration: String,
    labels: BTreeMap<String, String>,
    annotations: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Histogram bucket defaults
// ---------------------------------------------------------------------------

/// Returns default histogram buckets based on the metric name suffix.
fn default_buckets(name: &str) -> Vec<f64> {
    if name.ends_with("_duration_seconds") || name.ends_with("_seconds") {
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    } else if name.ends_with("_bytes") {
        vec![
            256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0,
        ]
    } else if name.ends_with("_count")
        || name.ends_with("_per_request")
        || name.ends_with("_per_tick")
    {
        vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]
    } else {
        // Fallback for histograms without a recognized suffix
        vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    }
}

/// Returns the effective buckets for a histogram metric.
fn effective_buckets(metric: &MetricDef) -> Vec<f64> {
    metric
        .buckets
        .clone()
        .unwrap_or_else(|| default_buckets(&metric.name))
}

// ---------------------------------------------------------------------------
// Dashboard generation
// ---------------------------------------------------------------------------

/// Determine the appropriate Grafana unit for a metric name.
fn grafana_unit(name: &str) -> &'static str {
    if name.ends_with("_seconds") || name.ends_with("_duration_seconds") {
        "s"
    } else if name.ends_with("_bytes") {
        "bytes"
    } else if name.ends_with("_total") {
        "ops"
    } else {
        "short"
    }
}

/// Build a Grafana timeseries panel.
fn timeseries_panel(
    id: u32,
    title: &str,
    targets: Vec<JsonValue>,
    unit: &str,
    grid_pos: JsonValue,
) -> JsonValue {
    json!({
        "id": id,
        "title": title,
        "type": "timeseries",
        "datasource": { "type": "prometheus", "uid": "${datasource}" },
        "gridPos": grid_pos,
        "fieldConfig": {
            "defaults": {
                "color": { "mode": "palette-classic" },
                "custom": {
                    "axisBorderShow": false,
                    "lineWidth": 2,
                    "fillOpacity": 10,
                    "spanNulls": false
                },
                "unit": unit
            },
            "overrides": []
        },
        "options": {
            "legend": { "displayMode": "list", "placement": "bottom" },
            "tooltip": { "mode": "multi" }
        },
        "targets": targets
    })
}

/// Build a Grafana stat panel.
fn stat_panel(
    id: u32,
    title: &str,
    targets: Vec<JsonValue>,
    unit: &str,
    grid_pos: JsonValue,
) -> JsonValue {
    json!({
        "id": id,
        "title": title,
        "type": "stat",
        "datasource": { "type": "prometheus", "uid": "${datasource}" },
        "gridPos": grid_pos,
        "fieldConfig": {
            "defaults": {
                "color": { "mode": "thresholds" },
                "thresholds": {
                    "mode": "absolute",
                    "steps": [
                        { "color": "green", "value": null }
                    ]
                },
                "unit": unit
            },
            "overrides": []
        },
        "options": {
            "colorMode": "background",
            "graphMode": "area",
            "justifyMode": "auto",
            "orientation": "auto",
            "reduceOptions": { "calcs": ["lastNotNull"], "fields": "", "values": false },
            "textMode": "auto"
        },
        "targets": targets
    })
}

/// Build a Grafana heatmap panel for histograms.
fn heatmap_panel(
    id: u32,
    title: &str,
    metric_name: &str,
    unit: &str,
    grid_pos: JsonValue,
) -> JsonValue {
    json!({
        "id": id,
        "title": title,
        "type": "heatmap",
        "datasource": { "type": "prometheus", "uid": "${datasource}" },
        "gridPos": grid_pos,
        "fieldConfig": {
            "defaults": {
                "unit": unit
            }
        },
        "options": {
            "calculate": false,
            "cellGap": 1,
            "color": {
                "mode": "scheme",
                "scheme": "Oranges"
            },
            "yAxis": { "unit": unit }
        },
        "targets": [
            {
                "expr": format!("sum(increase({}_bucket[$__rate_interval])) by (le)", metric_name),
                "legendFormat": "{{le}}",
                "refId": "A",
                "format": "heatmap"
            }
        ]
    })
}

/// Build a row panel.
fn row_panel(id: u32, title: &str, y: u32) -> JsonValue {
    json!({
        "id": id,
        "title": title,
        "type": "row",
        "collapsed": false,
        "gridPos": { "h": 1, "w": 24, "x": 0, "y": y }
    })
}

/// Build label-based template variables from a set of labels across all metrics.
fn build_template_variables(metrics: &[MetricDef]) -> Vec<JsonValue> {
    let mut label_set: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in metrics {
        for label in &m.labels {
            label_set
                .entry(label.clone())
                .or_default()
                .push(m.name.clone());
        }
    }

    let mut vars = vec![json!({
        "current": { "selected": false, "text": "Prometheus", "value": "Prometheus" },
        "hide": 0,
        "includeAll": false,
        "label": "Data Source",
        "multi": false,
        "name": "datasource",
        "options": [],
        "query": "prometheus",
        "refresh": 1,
        "type": "datasource"
    })];

    for (label, metric_names) in &label_set {
        // Use the first metric that has this label to populate the variable query
        let first_metric = &metric_names[0];
        vars.push(json!({
            "current": { "selected": true, "text": "All", "value": "$__all" },
            "definition": format!("label_values({}, {})", first_metric, label),
            "hide": 0,
            "includeAll": true,
            "label": label,
            "multi": true,
            "name": label,
            "query": format!("label_values({}, {})", first_metric, label),
            "refresh": 2,
            "sort": 1,
            "type": "query"
        }));
    }

    vars
}

/// Generate a full Grafana dashboard JSON for a single crate.
fn generate_dashboard(crate_info: &CrateInfo, metrics: &[MetricDef]) -> JsonValue {
    let mut panels: Vec<JsonValue> = Vec::new();
    let mut panel_id: u32 = 1;
    let mut y: u32 = 0;

    // Group metrics by their group field
    let mut groups: BTreeMap<String, Vec<&MetricDef>> = BTreeMap::new();
    for m in metrics {
        let group_name = m.group.clone().unwrap_or_else(|| "General".to_string());
        groups.entry(group_name).or_default().push(m);
    }

    for (group_name, group_metrics) in &groups {
        // Add row panel
        panels.push(row_panel(panel_id, group_name, y));
        panel_id += 1;
        y += 1;

        let mut x: u32 = 0;
        for metric in group_metrics {
            let unit = grafana_unit(&metric.name);

            match metric.metric_type.as_str() {
                "counter" => {
                    // rate() timeseries panel
                    let label_by = if metric.labels.is_empty() {
                        String::new()
                    } else {
                        format!(" by ({})", metric.labels.join(", "))
                    };
                    let legend = if metric.labels.is_empty() {
                        metric.help.clone()
                    } else {
                        metric
                            .labels
                            .iter()
                            .map(|l| format!("{{{{{}}}}}", l))
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    let targets = vec![json!({
                        "expr": format!("sum{}(rate({}[$__rate_interval]))", label_by, metric.name),
                        "legendFormat": legend,
                        "refId": "A"
                    })];

                    let width = if x + 12 <= 24 { 12 } else { 24 };
                    panels.push(timeseries_panel(
                        panel_id,
                        &metric.help,
                        targets,
                        "ops",
                        json!({ "h": 6, "w": width, "x": x % 24, "y": y }),
                    ));
                    panel_id += 1;
                    x += width;
                    if x >= 24 {
                        x = 0;
                        y += 6;
                    }
                }
                "histogram" => {
                    let _buckets = effective_buckets(metric);

                    // Heatmap panel
                    let heatmap_width = 12;
                    if x + heatmap_width > 24 {
                        x = 0;
                        y += 6;
                    }
                    panels.push(heatmap_panel(
                        panel_id,
                        &format!("{} (heatmap)", metric.help),
                        &metric.name,
                        unit,
                        json!({ "h": 6, "w": heatmap_width, "x": x % 24, "y": y }),
                    ));
                    panel_id += 1;
                    x += heatmap_width;

                    // p50/p95/p99 timeseries panel
                    let label_by = if metric.labels.is_empty() {
                        "le".to_string()
                    } else {
                        format!("le, {}", metric.labels.join(", "))
                    };
                    let ts_width = 12;
                    if x + ts_width > 24 {
                        x = 0;
                        y += 6;
                    }
                    let targets = vec![
                        json!({
                            "expr": format!("histogram_quantile(0.50, sum by ({}) (rate({}_bucket[$__rate_interval])))", label_by, metric.name),
                            "legendFormat": "p50",
                            "refId": "A"
                        }),
                        json!({
                            "expr": format!("histogram_quantile(0.95, sum by ({}) (rate({}_bucket[$__rate_interval])))", label_by, metric.name),
                            "legendFormat": "p95",
                            "refId": "B"
                        }),
                        json!({
                            "expr": format!("histogram_quantile(0.99, sum by ({}) (rate({}_bucket[$__rate_interval])))", label_by, metric.name),
                            "legendFormat": "p99",
                            "refId": "C"
                        }),
                    ];
                    panels.push(timeseries_panel(
                        panel_id,
                        &format!("{} (p50/p95/p99)", metric.help),
                        targets,
                        unit,
                        json!({ "h": 6, "w": ts_width, "x": x % 24, "y": y }),
                    ));
                    panel_id += 1;
                    x += ts_width;
                    if x >= 24 {
                        x = 0;
                        y += 6;
                    }
                }
                "gauge" => {
                    // Stat panel + timeseries panel
                    let label_by = if metric.labels.is_empty() {
                        String::new()
                    } else {
                        format!(" by ({})", metric.labels.join(", "))
                    };
                    let legend = if metric.labels.is_empty() {
                        metric.help.clone()
                    } else {
                        metric
                            .labels
                            .iter()
                            .map(|l| format!("{{{{{}}}}}", l))
                            .collect::<Vec<_>>()
                            .join(" ")
                    };

                    let stat_width = 6;
                    if x + stat_width > 24 {
                        x = 0;
                        y += 6;
                    }
                    let stat_targets = vec![json!({
                        "expr": format!("sum{}({})", label_by, metric.name),
                        "legendFormat": legend.clone(),
                        "refId": "A"
                    })];
                    panels.push(stat_panel(
                        panel_id,
                        &metric.help,
                        stat_targets,
                        unit,
                        json!({ "h": 6, "w": stat_width, "x": x % 24, "y": y }),
                    ));
                    panel_id += 1;
                    x += stat_width;

                    let ts_width = 6;
                    if x + ts_width > 24 {
                        x = 0;
                        y += 6;
                    }
                    let ts_targets = vec![json!({
                        "expr": format!("sum{}({})", label_by, metric.name),
                        "legendFormat": legend,
                        "refId": "A"
                    })];
                    panels.push(timeseries_panel(
                        panel_id,
                        &format!("{} (over time)", metric.help),
                        ts_targets,
                        unit,
                        json!({ "h": 6, "w": ts_width, "x": x % 24, "y": y }),
                    ));
                    panel_id += 1;
                    x += ts_width;
                    if x >= 24 {
                        x = 0;
                        y += 6;
                    }
                }
                _ => {}
            }
        }

        // Move to next row if current row has content
        if x > 0 {
            y += 6;
        }
    }

    let template_vars = build_template_variables(metrics);
    let uid = crate_info.name.replace('-', "_");

    json!({
        "annotations": {
            "list": [{
                "builtIn": 1,
                "datasource": { "type": "grafana", "uid": "-- Grafana --" },
                "enable": true,
                "hide": true,
                "iconColor": "rgba(0, 211, 255, 1)",
                "name": "Annotations & Alerts",
                "type": "dashboard"
            }]
        },
        "editable": true,
        "fiscalYearStartMonth": 0,
        "graphTooltip": 1,
        "id": null,
        "links": [],
        "panels": panels,
        "schemaVersion": 39,
        "tags": ["sober", "generated", &crate_info.name],
        "templating": { "list": template_vars },
        "time": { "from": "now-1h", "to": "now" },
        "timepicker": {},
        "timezone": "browser",
        "title": crate_info.dashboard_title.clone(),
        "uid": uid,
        "version": 1
    })
}

// ---------------------------------------------------------------------------
// Alert rule generation
// ---------------------------------------------------------------------------

/// Generate Prometheus alert rules from all `[[metrics.alerts]]` sections in a crate.
fn generate_alert_rules(crate_info: &CrateInfo, metrics: &[MetricDef]) -> Option<AlertRuleFile> {
    let mut rules = Vec::new();

    for metric in metrics {
        for alert in &metric.alerts {
            // Expand {{name}} placeholder with the metric name
            let expr = alert.expr.replace("{{name}}", &metric.name);

            let mut labels = BTreeMap::new();
            labels.insert("severity".to_string(), alert.severity.clone());

            let mut annotations = BTreeMap::new();
            annotations.insert("summary".to_string(), alert.summary.clone());
            annotations.insert(
                "description".to_string(),
                format!(
                    "Alert from {} metric {} in crate {}",
                    alert.severity, metric.name, crate_info.name
                ),
            );

            rules.push(AlertRule {
                alert: alert.name.clone(),
                expr,
                for_duration: alert.for_duration.clone(),
                labels,
                annotations,
            });
        }
    }

    if rules.is_empty() {
        return None;
    }

    Some(AlertRuleFile {
        groups: vec![AlertGroup {
            name: format!("{}_generated", crate_info.name.replace('-', "_")),
            rules,
        }],
    })
}

// ---------------------------------------------------------------------------
// Discovery and main
// ---------------------------------------------------------------------------

/// Discover all `metrics.toml` files under the input directory.
fn discover_metrics_files(input: &PathBuf) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !input.is_dir() {
        anyhow::bail!("Input path is not a directory: {}", input.display());
    }

    for entry in fs::read_dir(input).context("Failed to read input directory")? {
        let entry = entry?;
        let metrics_path = entry.path().join("metrics.toml");
        if metrics_path.exists() {
            files.push(metrics_path);
        }
    }

    files.sort();
    Ok(files)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Create output directories if they don't exist
    fs::create_dir_all(&cli.dashboards_output)
        .context("Failed to create dashboards output directory")?;
    fs::create_dir_all(&cli.alerts_output)
        .context("Failed to create alerts output directory")?;

    let files = discover_metrics_files(&cli.input)?;

    if files.is_empty() {
        eprintln!("No metrics.toml files found in {}", cli.input.display());
        return Ok(());
    }

    let mut dashboard_count = 0;
    let mut alert_count = 0;

    for path in &files {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let metrics_file = parse_metrics_file(&content, path)?;

        // Generate dashboard JSON
        let dashboard = generate_dashboard(&metrics_file.crate_info, &metrics_file.metrics);
        let dashboard_filename = format!("{}.json", metrics_file.crate_info.name);
        let dashboard_path = cli.dashboards_output.join(&dashboard_filename);
        let dashboard_json = serde_json::to_string_pretty(&dashboard)
            .context("Failed to serialize dashboard JSON")?;
        fs::write(&dashboard_path, &dashboard_json)
            .with_context(|| format!("Failed to write {}", dashboard_path.display()))?;
        dashboard_count += 1;
        eprintln!(
            "  Dashboard: {} -> {}",
            metrics_file.crate_info.name,
            dashboard_path.display()
        );

        // Generate alert rules YAML
        if let Some(alert_rules) =
            generate_alert_rules(&metrics_file.crate_info, &metrics_file.metrics)
        {
            let alert_filename = format!("{}.yml", metrics_file.crate_info.name);
            let alert_path = cli.alerts_output.join(&alert_filename);
            let alert_yaml =
                serde_yaml::to_string(&alert_rules).context("Failed to serialize alert YAML")?;
            fs::write(&alert_path, &alert_yaml)
                .with_context(|| format!("Failed to write {}", alert_path.display()))?;
            alert_count += 1;
            eprintln!(
                "  Alerts:    {} -> {}",
                metrics_file.crate_info.name,
                alert_path.display()
            );
        }
    }

    eprintln!(
        "Generated {} dashboards and {} alert rule files from {} metrics.toml files.",
        dashboard_count,
        alert_count,
        files.len()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_METRICS_TOML: &str = r#"
[crate]
name = "sober-llm"
dashboard_title = "LLM Engine"

[[metrics]]
name = "sober_llm_request_total"
type = "counter"
help = "Total LLM API requests"
labels = ["provider", "model", "status"]
group = "Requests"

[[metrics]]
name = "sober_llm_request_duration_seconds"
type = "histogram"
help = "LLM request latency"
labels = ["provider", "model"]
group = "Requests"
buckets = [0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]

  [[metrics.alerts]]
  name = "LLMLatencyDegraded"
  severity = "warning"
  expr = "histogram_quantile(0.95, rate({{name}}_bucket[5m])) > 15"
  for = "5m"
  summary = "LLM p95 latency above 15s"

[[metrics]]
name = "sober_llm_tokens_input_total"
type = "counter"
help = "Total input tokens consumed"
labels = ["provider", "model"]
group = "Tokens"

[[metrics]]
name = "sober_llm_embed_dimensions"
type = "gauge"
help = "Embedding vector dimensions"
labels = []
group = "Embeddings"
"#;

    #[test]
    fn test_parse_metrics_toml() {
        let metrics_file: MetricsFile =
            toml::from_str(SAMPLE_METRICS_TOML).expect("Failed to parse sample metrics.toml");

        assert_eq!(metrics_file.crate_info.name, "sober-llm");
        assert_eq!(metrics_file.crate_info.dashboard_title, "LLM Engine");
        assert_eq!(metrics_file.metrics.len(), 4);

        // Check counter
        let counter = &metrics_file.metrics[0];
        assert_eq!(counter.name, "sober_llm_request_total");
        assert_eq!(counter.metric_type, "counter");
        assert_eq!(counter.labels, vec!["provider", "model", "status"]);

        // Check histogram with custom buckets
        let hist = &metrics_file.metrics[1];
        assert_eq!(hist.metric_type, "histogram");
        assert_eq!(
            hist.buckets,
            Some(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0])
        );
        assert_eq!(hist.alerts.len(), 1);
        assert_eq!(hist.alerts[0].name, "LLMLatencyDegraded");

        // Check gauge
        let gauge = &metrics_file.metrics[3];
        assert_eq!(gauge.metric_type, "gauge");
        assert!(gauge.labels.is_empty());
    }

    #[test]
    fn test_generate_dashboard_json() {
        let metrics_file: MetricsFile =
            toml::from_str(SAMPLE_METRICS_TOML).expect("Failed to parse");
        let dashboard =
            generate_dashboard(&metrics_file.crate_info, &metrics_file.metrics);

        // Verify it's valid JSON
        let json_str =
            serde_json::to_string_pretty(&dashboard).expect("Failed to serialize dashboard");
        let reparsed: JsonValue =
            serde_json::from_str(&json_str).expect("Failed to reparse dashboard JSON");

        // Check top-level fields
        assert_eq!(reparsed["title"], "LLM Engine");
        assert_eq!(reparsed["uid"], "sober_llm");
        assert_eq!(reparsed["schemaVersion"], 39);

        // Check panels exist
        let panels = reparsed["panels"].as_array().expect("panels should be array");
        assert!(!panels.is_empty(), "Should have generated panels");

        // Check we have row panels for groups
        let row_panels: Vec<_> = panels.iter().filter(|p| p["type"] == "row").collect();
        assert!(
            row_panels.len() >= 3,
            "Should have at least 3 row panels (Embeddings, Requests, Tokens)"
        );

        // Check template variables include label-based variables
        let template_vars = reparsed["templating"]["list"]
            .as_array()
            .expect("template list should be array");
        assert!(
            template_vars.len() > 1,
            "Should have datasource + label variables"
        );

        // Check datasource variable exists
        assert_eq!(template_vars[0]["name"], "datasource");
    }

    #[test]
    fn test_generate_alert_rules_yaml() {
        let metrics_file: MetricsFile =
            toml::from_str(SAMPLE_METRICS_TOML).expect("Failed to parse");
        let alert_rules =
            generate_alert_rules(&metrics_file.crate_info, &metrics_file.metrics);

        let alert_rules = alert_rules.expect("Should have generated alert rules");
        assert_eq!(alert_rules.groups.len(), 1);
        assert_eq!(alert_rules.groups[0].name, "sober_llm_generated");
        assert_eq!(alert_rules.groups[0].rules.len(), 1);

        let rule = &alert_rules.groups[0].rules[0];
        assert_eq!(rule.alert, "LLMLatencyDegraded");
        assert_eq!(rule.for_duration, "5m");
        assert_eq!(rule.labels["severity"], "warning");

        // Check {{name}} was expanded
        assert!(
            rule.expr
                .contains("sober_llm_request_duration_seconds_bucket"),
            "PromQL should have {{{{name}}}} expanded. Got: {}",
            rule.expr
        );

        // Verify YAML serialization works
        let yaml_str =
            serde_yaml::to_string(&alert_rules).expect("Failed to serialize alert rules YAML");
        assert!(yaml_str.contains("LLMLatencyDegraded"));
        assert!(yaml_str.contains("severity: warning"));
    }

    #[test]
    fn test_no_alerts_returns_none() {
        let toml_str = r#"
[crate]
name = "sober-core"
dashboard_title = "Core"

[[metrics]]
name = "sober_core_uptime_seconds"
type = "gauge"
help = "Process uptime"
"#;
        let metrics_file: MetricsFile = toml::from_str(toml_str).expect("Failed to parse");
        let result = generate_alert_rules(&metrics_file.crate_info, &metrics_file.metrics);
        assert!(result.is_none(), "Should return None when no alerts defined");
    }

    #[test]
    fn test_histogram_bucket_defaults() {
        // Duration seconds suffix
        let buckets = default_buckets("sober_api_request_duration_seconds");
        assert_eq!(
            buckets,
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
        );

        // Generic _seconds suffix also matches
        let buckets = default_buckets("sober_crypto_sign_duration_seconds");
        assert_eq!(
            buckets,
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
        );

        // Bytes suffix
        let buckets = default_buckets("sober_memory_bcf_size_bytes");
        assert_eq!(
            buckets,
            vec![256.0, 1024.0, 4096.0, 16384.0, 65536.0, 262144.0, 1048576.0, 4194304.0]
        );

        // Count suffix
        let buckets = default_buckets("sober_memory_search_results_count");
        assert_eq!(buckets, vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);

        // Per-request suffix
        let buckets = default_buckets("sober_llm_tokens_per_request");
        assert_eq!(buckets, vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);

        // Per-tick suffix
        let buckets = default_buckets("sober_scheduler_jobs_due_per_tick");
        assert_eq!(buckets, vec![1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0]);

        // Unknown suffix falls back to duration default
        let buckets = default_buckets("sober_some_unknown_histogram");
        assert_eq!(
            buckets,
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
        );
    }

    #[test]
    fn test_effective_buckets_uses_custom_when_specified() {
        let metric = MetricDef {
            name: "sober_llm_request_duration_seconds".to_string(),
            metric_type: "histogram".to_string(),
            help: "LLM latency".to_string(),
            labels: vec![],
            group: None,
            buckets: Some(vec![0.1, 0.5, 1.0, 5.0]),
            alerts: vec![],
        };
        let buckets = effective_buckets(&metric);
        assert_eq!(buckets, vec![0.1, 0.5, 1.0, 5.0]);
    }

    #[test]
    fn test_effective_buckets_uses_default_when_unspecified() {
        let metric = MetricDef {
            name: "sober_api_request_duration_seconds".to_string(),
            metric_type: "histogram".to_string(),
            help: "API latency".to_string(),
            labels: vec![],
            group: None,
            buckets: None,
            alerts: vec![],
        };
        let buckets = effective_buckets(&metric);
        assert_eq!(
            buckets,
            vec![0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
        );
    }

    #[test]
    fn test_grafana_unit_detection() {
        assert_eq!(grafana_unit("sober_api_request_duration_seconds"), "s");
        assert_eq!(grafana_unit("sober_memory_bcf_size_bytes"), "bytes");
        assert_eq!(grafana_unit("sober_llm_request_total"), "ops");
        assert_eq!(grafana_unit("sober_scheduler_paused"), "short");
    }
}
