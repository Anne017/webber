use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub struct Package {
    pub url: String,
    pub name: String,
    pub theme_color: String,
    pub icon_url: String,
    pub url_patterns: String,
}

impl Package {
    fn appname(&self) -> String {
        let url_part = url::Url::parse(&self.url)
            .ok()
            .map(|url| url.host_str().map(String::from))
            .map(|url| url.unwrap_or_else(|| self.url.clone()))
            .unwrap_or_else(|| self.url.clone());
        // Remove forbidden characters
        let ascii = url_part.to_ascii_lowercase();
        let allowed_chars = ascii
            .chars()
            .filter_map(|c| {
                if c == '.' || c == '_' {
                    Some('-')
                } else if ('a'..'z').contains(&c) || c.is_digit(10) {
                    Some(c)
                } else {
                    None
                }
            })
            .collect::<String>();
        format!("webapp-{}", allowed_chars)
    }
}

pub fn create_package(package: Package) -> Result<(), Box<dyn std::error::Error>> {
    let path = xdg::BaseDirectories::new()?
        .get_cache_home()
        .join("webber.timsueberkrueb/click-build");
    fs::create_dir_all(&path)?;
    // Clean up
    fs::remove_dir_all(&path)?;
    fs::create_dir(&path)?;

    let control = path.join(Path::new("control"));
    let data = path.join(Path::new("data"));

    mkdir(&control)?;
    mkdir(&data)?;

    let click_binary = path.join(Path::new("click_binary"));
    let debian_binary = path.join(Path::new("debian-binary"));

    write_file(&click_binary, "0.4\n")?;
    write_file(&debian_binary, "2.0\n")?;
    write_file(
        &control.join(Path::new("control")),
        &control_control_content(&package.appname()),
    )?;
    write_file(
        &control.join(Path::new("manifest")),
        &control_manifest_content(&package.appname(), &package.name),
    )?;
    write_file(&data.join(Path::new("preinst")), control_preinst_content())?;

    // TODO: md5sums
    write_file(
        &data.join(Path::new("shortcut.apparmor")),
        data_apparmor_content(),
    )?;

    let ext = url::Url::parse(&package.icon_url)
        .ok()
        .map(|icon| Some(icon.path_segments()?.map(String::from).collect::<Vec<_>>()))
        .map(|segments| segments?.iter().rev().cloned().next())
        .map(|last| last?.rsplit('.').map(String::from).next())
        .unwrap_or_default();

    let icon_filename = if let Some(ext) = ext {
        let icon_fname = format!("icon.{}", ext);
        download_file(package.icon_url, &data.join(Path::new(&icon_fname)))?;
        icon_fname
    } else {
        let icon_fname = "icon.svg".to_owned();
        write_icon(&data.join(Path::new(&icon_fname)))?;
        icon_fname
    };

    write_file(
        &data.join(Path::new("shortcut.desktop")),
        &data_desktop_content(
            &package.name,
            &package.url,
            &package.url_patterns,
            &icon_filename,
            &package.theme_color,
        ),
    )?;

    let control_tar_gz = path.join(Path::new("control.tar.gz"));
    let data_tar_gz = path.join(Path::new("data.tar.gz"));

    create_tar_gz(&control_tar_gz, &control)?;
    create_tar_gz(&data_tar_gz, &data)?;

    let click_path = path.join(Path::new("shortcut.click"));

    create_ar(
        &click_path,
        &[
            (&debian_binary, "debian-binary"),
            (&control_tar_gz, "control.tar.gz"),
            (&data_tar_gz, "data.tar.gz"),
            (&click_binary, "_click-binary"),
        ],
    )?;

    Ok(())
}

fn download_file(url: String, target: &Path) -> Result<(), Box<dyn Error>> {
    let mut resp = reqwest::get(&url)?;
    let mut file = fs::File::create(target)?;
    io::copy(&mut resp, &mut file)?;
    Ok(())
}

fn create_ar(filepath: &Path, files: &[(&Path, &str)]) -> io::Result<()> {
    let file = fs::File::create(filepath)?;
    let mut archive = ar::Builder::new(file);
    for (src, target) in files {
        let mut file = fs::File::open(src)?;
        archive.append_file(&target.as_bytes(), &mut file)?;
    }
    Ok(())
}

fn create_tar_gz(filepath: &Path, dir: &Path) -> io::Result<()> {
    // FIXME: We cannot use the `tar` crate as for some reason the filepaths
    // need to start with ./ and this seem to get normalized away in Rust paths.
    // This workaround should be okay because we control the filepath, but it is ugly
    // nevertheless.
    Command::new("tar")
        .args(&[
            "--transform",
            &format!(
                "flags=r;s|{}|.|",
                dir.file_name().unwrap().to_str().unwrap()
            ),
            "-czf",
            filepath.to_str().unwrap(),
            dir.file_name().unwrap().to_str().unwrap(),
        ])
        .current_dir(&dir.join(".."))
        .output()?;
    Ok(())
}

fn mkdir(dirname: &Path) -> io::Result<()> {
    fs::create_dir(dirname)
}

fn write_file(filename: &Path, content: &str) -> io::Result<()> {
    let mut file = fs::File::create(filename)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

fn control_control_content(appname: &str) -> String {
    format!(
        r#"Package: {}.webber
Version: 1.0.0
Click-Version: 0.4
Architecture: all
Maintainer: Webber <noreply@ubports.com>
Description: Shortcut
"#,
        appname,
    )
}

fn control_manifest_content(appname: &str, title: &str) -> String {
    format!(
        r#"{{
    "architecture": "all",
    "description": "Shortcut",
    "framework": "ubuntu-sdk-16.04",
    "hooks": {{
        "{}.webber": {{
            "apparmor": "shortcut.apparmor",
            "desktop": "shortcut.desktop"
        }}
    }},
    "installed-size": "30",
    "maintainer": "Webber <noreply@ubports.com>",
    "name": "{}.webber",
    "title": "{}",
    "version": "1.0.0"
}}
"#,
        appname, appname, title,
    )
}

fn control_preinst_content() -> &'static str {
    r#"#! /bin/sh
echo "Click packages may not be installed directly using dpkg."
echo "Use 'click install' instead."
exit 1"#
}

fn data_apparmor_content() -> &'static str {
    r#"{
    "template": "ubuntu-webapp",
    "policy_groups": [
        "networking",
        "webview"
    ],
    "policy_version": 16.04
}
"#
}

fn data_desktop_content(
    title: &str,
    url: &str,
    url_patterns: &str,
    icon_fname: &str,
    theme_color: &str,
) -> String {
    format!(
        r#"[Desktop Entry]
Name={}
Exec=webapp-container --webappUrlPatterns={} --store-session-cookies {}
Icon={}
Terminal=false
Type=Application
X-Ubuntu-Touch=true
X-Ubuntu-Splash-Color={}
"#,
        title, url_patterns, url, icon_fname, theme_color
    )
}

fn write_icon(path: &Path) -> io::Result<()> {
    let bytes = include_bytes!("../assets/logo.svg");
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    Ok(())
}
