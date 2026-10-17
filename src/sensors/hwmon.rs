//! Linux hwmon (/sys/class/hwmon): CPU package temperature and other chips.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HwmonTemp {
    pub chip: String,
    pub label: String,
    pub celsius: f32,
}

const CPU_CHIPS: [&str; 3] = ["k10temp", "zenpower", "coretemp"];

/// Reads every `temp*_input` of every hwmon chip.
pub fn read_all(root: &Path) -> Vec<HwmonTemp> {
    let mut temps = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return temps;
    };
    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let Ok(chip) = std::fs::read_to_string(dir.join("name")) else {
            continue;
        };
        let chip = chip.trim().to_string();
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut inputs: Vec<String> = files
            .flatten()
            .filter_map(|f| f.file_name().into_string().ok())
            .filter(|n| n.starts_with("temp") && n.ends_with("_input"))
            .collect();
        inputs.sort_by_key(|n| n.trim_start_matches("temp").trim_end_matches("_input").parse::<u32>().unwrap_or(0));
        for input in inputs {
            let index = input.trim_end_matches("_input");
            let Some(millis) =
                std::fs::read_to_string(dir.join(&input)).ok().and_then(|v| v.trim().parse::<i64>().ok())
            else {
                continue;
            };
            let label = std::fs::read_to_string(dir.join(format!("{index}_label")))
                .map(|l| l.trim().to_string())
                .unwrap_or_else(|_| index.to_string());
            temps.push(HwmonTemp { chip: chip.clone(), label, celsius: millis as f32 / 1000.0 });
        }
    }
    temps
}

/// CPU temperature: package sensor (Tdie/Tctl on AMD, Package id on Intel), or the hottest core.
/// With several sockets, the hottest one.
pub fn cpu_temperature(temps: &[HwmonTemp]) -> Option<f32> {
    let cpu: Vec<&HwmonTemp> = temps.iter().filter(|t| CPU_CHIPS.contains(&t.chip.as_str())).collect();
    let max = |prefixes: &[&str]| {
        cpu.iter().filter(|t| prefixes.iter().any(|p| t.label.starts_with(p))).map(|t| t.celsius).reduce(f32::max)
    };
    max(&["Tdie"]).or_else(|| max(&["Tctl", "Package id"])).or_else(|| max(&["Tccd", "Core"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, file: &str, content: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(file), content).unwrap();
    }

    #[test]
    fn reads_amd_and_nvme() {
        let root = tempfile::tempdir().unwrap();
        let k10 = root.path().join("hwmon0");
        write(&k10, "name", "k10temp\n");
        write(&k10, "temp1_input", "45250\n");
        write(&k10, "temp1_label", "Tctl\n");
        write(&k10, "temp3_input", "40000\n");
        write(&k10, "temp3_label", "Tccd1\n");
        let nvme = root.path().join("hwmon1");
        write(&nvme, "name", "nvme\n");
        write(&nvme, "temp1_input", "38850\n");
        write(&nvme, "temp1_label", "Composite\n");

        let temps = read_all(root.path());
        assert_eq!(temps.len(), 3);
        assert_eq!(cpu_temperature(&temps), Some(45.25));
        assert_eq!(temps[2], HwmonTemp { chip: "nvme".into(), label: "Composite".into(), celsius: 38.85 });
    }

    #[test]
    fn intel_dual_socket_takes_hottest_package() {
        let t = |label: &str, c: f32| HwmonTemp { chip: "coretemp".into(), label: label.into(), celsius: c };
        let temps = vec![t("Package id 0", 50.0), t("Core 0", 55.0), t("Package id 1", 61.0)];
        assert_eq!(cpu_temperature(&temps), Some(61.0));
    }

    #[test]
    fn falls_back_to_cores() {
        let t = |label: &str, c: f32| HwmonTemp { chip: "coretemp".into(), label: label.into(), celsius: c };
        assert_eq!(cpu_temperature(&[t("Core 0", 40.0), t("Core 1", 44.0)]), Some(44.0));
        assert_eq!(cpu_temperature(&[]), None);
    }
}
