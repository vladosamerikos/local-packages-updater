use flate2::read::GzDecoder;
use reqwest::{header::{ETAG, IF_NONE_MATCH, ACCEPT}, Client, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};
use xz2::read::XzDecoder;

const REPOSITORY: &str = "https://persistent.oaistatic.com/codex-app-prod/linux/deb";
const KEYRING: &str = "/usr/share/keyrings/chatgpt-archive-keyring.gpg";
const RELEASES: &str = "https://api.github.com/repos/crmne/spotifast/releases?per_page=30";
const MAX_DOWNLOAD: usize = 5_000_000;
const MAX_INDEX: usize = 25_000_000;

#[derive(Clone)]
pub struct Checkers { client: Client, cache: PathBuf }
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub installed: String,
    pub latest: String,
    pub update_available: bool,
}
#[derive(Serialize, Deserialize)]
struct CachedResponse { etag: Option<String>, data: Vec<u8> }
#[derive(Debug)]
struct IndexEntry { path: String, hash: String, size: usize }
#[derive(Deserialize)]
struct Release { tag_name: String, draft: bool, prerelease: bool, assets: Vec<Asset> }
#[derive(Deserialize)]
struct Asset { name: String }

impl Checkers {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("actualizaciones/", env!("CARGO_PKG_VERSION")))
            .https_only(true).connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::limited(5))
            .build().map_err(|_| "No se pudo iniciar el cliente de red".to_string())?;
        let root = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        Ok(Self { client, cache: root.join("actualizaciones") })
    }
    pub async fn check(&self, id: &str) -> Result<CheckResult, String> {
        match id {
            "codex-desktop" => self.codex().await,
            "spotifast" => self.spotifast().await,
            _ => Err("Aplicación desconocida".to_string()),
        }
    }
    async fn cached_get(&self, url: &str, app: &str, name: &str) -> Result<Vec<u8>, String> {
        let path = self.cache.join(app).join(format!("{name}-http.json"));
        let cached = fs::read(&path).ok()
            .and_then(|bytes| serde_json::from_slice::<CachedResponse>(&bytes).ok())
            .filter(|response| response.data.len() <= MAX_DOWNLOAD);
        let mut request = self.client.get(url).header(ACCEPT, "application/vnd.github+json");
        if let Some(etag) = cached.as_ref().and_then(|response| response.etag.as_ref()) {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(etag) {
                request = request.header(IF_NONE_MATCH, value);
            }
        }
        let response = request.send().await.map_err(network_error)?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return cached.map(|response| response.data)
                .ok_or_else(|| "No hay una caché válida para esta respuesta".to_string());
        }
        let status = response.status();
        if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
            return Err("El servidor ha limitado las consultas. Vuelve a intentarlo más tarde.".to_string());
        }
        if !status.is_success() { return Err(format!("El servidor respondió con un error HTTP {}", status.as_u16())); }
        let etag = response.headers().get(ETAG).and_then(|value| value.to_str().ok()).map(str::to_string);
        let data = limited_body(response).await?;
        let bytes = serde_json::to_vec(&CachedResponse { etag, data: data.clone() })
            .map_err(|_| "No se pudo guardar la respuesta".to_string())?;
        if let Err(error) = atomic_write(&path, &bytes) { eprintln!("No se pudo guardar la caché: {error}"); }
        Ok(data)
    }
    async fn codex(&self) -> Result<CheckResult, String> {
        let package = local_output("/usr/bin/dpkg-query", &["-W", "-f=${Status}\t${Version}", "chatgpt"]).await
            .map_err(|_| "Codex Desktop no está instalado como paquete APT chatgpt".to_string())?;
        let installed = package.strip_prefix("install ok installed\t").map(str::trim)
            .filter(|version| valid_debian_version(version))
            .ok_or_else(|| "No se encontró la versión instalada de Codex Desktop".to_string())?.to_string();
        if !Path::new(KEYRING).is_file() { return Err("No se encontró la clave de firma del repositorio Codex".to_string()); }
        let signed = self.cached_get(&format!("{REPOSITORY}/dists/stable/InRelease"), "codex-desktop", "inrelease").await?;
        let temporary = tempfile::tempdir().map_err(|_| "No se pudo crear un archivo temporal".to_string())?;
        let signed_path = temporary.path().join("InRelease");
        let release_path = temporary.path().join("Release");
        fs::write(&signed_path, signed).map_err(|_| "No se pudo leer el índice firmado".to_string())?;
        local_output("/usr/bin/gpgv", &["--keyring", KEYRING, "--output",
            release_path.to_str().ok_or("Ruta temporal no válida")?,
            signed_path.to_str().ok_or("Ruta temporal no válida")?]).await
            .map_err(|_| "La firma del índice del repositorio Codex no es válida".to_string())?;
        let release = fs::read_to_string(&release_path).map_err(|_| "No se pudo leer el manifiesto de Codex".to_string())?;
        let entry = parse_index_entry(&release)?;
        let index_path = self.cache.join("codex-desktop").join(format!("Packages-{}", entry.hash));
        let cached = fs::read(&index_path).ok().filter(|bytes| verify_index(bytes, &entry).is_ok());
        let bytes = if let Some(bytes) = cached { bytes } else {
            let response = self.client.get(format!("{REPOSITORY}/dists/stable/{}", entry.path))
                .send().await.map_err(network_error)?.error_for_status().map_err(network_error)?;
            let bytes = limited_body(response).await?;
            verify_index(&bytes, &entry)?;
            let _ = atomic_write(&index_path, &bytes);
            bytes
        };
        verify_index(&bytes, &entry)?;
        let packages = decompress_index(&bytes, &entry.path)?;
        let mut latest: Option<String> = None;
        for candidate in package_versions(&packages) {
            if latest.is_none() || newer(&candidate, latest.as_ref().unwrap()).await? { latest = Some(candidate); }
        }
        let latest = latest.ok_or_else(|| "No se encontró una versión amd64 válida de Codex Desktop".to_string())?;
        let update_available = newer(&latest, &installed).await?;
        Ok(CheckResult { installed, latest, update_available })
    }
    async fn spotifast(&self) -> Result<CheckResult, String> {
        let applications = local_output("/usr/bin/flatpak", &["list", "--user", "--app", "--columns=application,version"])
            .await.map_err(|_| "No se pudo consultar la instalación de Spotifast".to_string())?;
        let installed = applications.lines().filter_map(|line| line.split_once('\t'))
            .find(|(id, _)| *id == "rocks.spotifast.Spotifast")
            .map(|(_, version)| version.trim().to_string())
            .ok_or_else(|| "Spotifast no está instalado para este usuario".to_string())?;
        if !valid_installed_semver(&installed) { return Err("No se pudo leer una versión válida de Spotifast".to_string()); }
        let bytes = self.cached_get(RELEASES, "spotifast", "releases").await?;
        let releases: Vec<Release> = serde_json::from_slice(&bytes)
            .map_err(|_| "GitHub devolvió una lista de versiones no válida".to_string())?;
        let latest = select_spotifast_release(&releases)?;
        let update_available = newer(&latest, &installed).await?;
        Ok(CheckResult { installed, latest, update_available })
    }
}

async fn limited_body(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|length| length > MAX_DOWNLOAD as u64) {
        return Err("La respuesta del servidor es demasiado grande".to_string());
    }
    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if data.len().saturating_add(chunk.len()) > MAX_DOWNLOAD {
            return Err("La respuesta del servidor es demasiado grande".to_string());
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
fn network_error(error: reqwest::Error) -> String {
    eprintln!("Error de red: {error}");
    if error.is_timeout() { "El servidor tarda demasiado en responder. Vuelve a verificar.".to_string() }
    else { "No se pudo conectar con el servidor. Comprueba tu conexión.".to_string() }
}
async fn local_output(program: &str, args: &[&str]) -> Result<String, String> {
    let child = Command::new(program).args(args).stdin(Stdio::null())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn()
        .map_err(|_| format!("No se pudo iniciar {program}"))?;
    let output = timeout(Duration::from_secs(15), child.wait_with_output()).await
        .map_err(|_| "La consulta local superó el tiempo límite".to_string())?
        .map_err(|_| "La consulta local no se pudo completar".to_string())?;
    if !output.status.success() {
        eprintln!("Error en {program}: {}", String::from_utf8_lossy(&output.stderr));
        return Err("La consulta local terminó con error".to_string());
    }
    String::from_utf8(output.stdout).map_err(|_| "La respuesta local no es válida".to_string())
}
async fn newer(latest: &str, installed: &str) -> Result<bool, String> {
    let child = Command::new("/usr/bin/dpkg").args(["--compare-versions", latest, "gt", installed])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true)
        .spawn().map_err(|_| "No se pudo comparar la versión instalada".to_string())?;
    let output = timeout(Duration::from_secs(5), child.wait_with_output()).await
        .map_err(|_| "La comparación de versiones superó el tiempo límite".to_string())?
        .map_err(|_| "No se pudo comparar la versión instalada".to_string())?;
    match output.status.code() {
        Some(0) => Ok(true), Some(1) => Ok(false),
        _ => Err("El formato de la versión no es válido".to_string()),
    }
}
fn valid_debian_version(version: &str) -> bool {
    !version.is_empty() && version.len() <= 128 && version.as_bytes()[0].is_ascii_digit() &&
        version.bytes().all(|byte| byte.is_ascii_alphanumeric() || b".+:~_-".contains(&byte))
}
fn semver(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let parse = |part: Option<&str>| -> Option<u64> {
        let part = part?;
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
        part.parse().ok()
    };
    let result = (parse(parts.next())?, parse(parts.next())?, parse(parts.next())?);
    if parts.next().is_some() { None } else { Some(result) }
}
fn valid_installed_semver(version: &str) -> bool {
    semver(version.split(['-', '+']).next().unwrap_or("")).is_some() && version.len() <= 128 &&
        version.bytes().all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte))
}
fn parse_index_entry(release: &str) -> Result<IndexEntry, String> {
    let mut in_sha256 = false;
    let mut entries = Vec::new();
    for line in release.lines() {
        if line == "SHA256:" { in_sha256 = true; continue; }
        if !in_sha256 { continue; }
        if !line.starts_with(char::is_whitespace) { break; }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 { continue; }
        if !["main/binary-amd64/Packages.xz", "main/binary-amd64/Packages.gz", "main/binary-amd64/Packages"].contains(&fields[2]) { continue; }
        if fields[0].len() != 64 || !fields[0].bytes().all(|byte| byte.is_ascii_hexdigit()) { continue; }
        let Ok(size) = fields[1].parse::<usize>() else { continue; };
        if size == 0 || size > MAX_DOWNLOAD { continue; }
        entries.push(IndexEntry { path: fields[2].to_string(), hash: fields[0].to_ascii_lowercase(), size });
    }
    entries.into_iter().min_by_key(|entry| if entry.path.ends_with(".xz") { 0 } else if entry.path.ends_with(".gz") { 1 } else { 2 })
        .ok_or_else(|| "El manifiesto firmado no contiene el índice amd64 esperado".to_string())
}
fn verify_index(bytes: &[u8], entry: &IndexEntry) -> Result<(), String> {
    if bytes.len() != entry.size || format!("{:x}", Sha256::digest(bytes)) != entry.hash {
        Err("El índice de paquetes no coincide con el manifiesto firmado".to_string())
    } else { Ok(()) }
}
fn decompress_index(bytes: &[u8], path: &str) -> Result<String, String> {
    let reader: Box<dyn Read + '_> = if path.ends_with(".gz") { Box::new(GzDecoder::new(bytes)) }
        else if path.ends_with(".xz") { Box::new(XzDecoder::new(bytes)) } else { Box::new(bytes) };
    let mut result = String::new();
    reader.take(MAX_INDEX as u64 + 1).read_to_string(&mut result)
        .map_err(|_| "No se pudo descomprimir el índice de paquetes".to_string())?;
    if result.len() > MAX_INDEX { return Err("El índice de paquetes es demasiado grande".to_string()); }
    Ok(result)
}
fn package_versions(packages: &str) -> Vec<String> {
    packages.replace("\r\n", "\n").split("\n\n").filter_map(|paragraph| {
        let mut package = None; let mut architecture = None; let mut version = None;
        for line in paragraph.lines() {
            if let Some(value) = line.strip_prefix("Package: ") { package = Some(value.trim()); }
            if let Some(value) = line.strip_prefix("Architecture: ") { architecture = Some(value.trim()); }
            if let Some(value) = line.strip_prefix("Version: ") { version = Some(value.trim()); }
        }
        if package == Some("chatgpt") && architecture == Some("amd64") {
            version.filter(|value| valid_debian_version(value)).map(str::to_string)
        } else { None }
    }).collect()
}
fn select_spotifast_release(releases: &[Release]) -> Result<String, String> {
    releases.iter().filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = release.tag_name.strip_prefix('v')?;
            let parsed = semver(version)?;
            let filename = format!("spotifast-{}-x86_64.flatpak", release.tag_name);
            if release.assets.iter().any(|asset| asset.name == filename) &&
                release.assets.iter().any(|asset| asset.name == "checksums.txt") {
                Some((parsed, version.to_string()))
            } else { None }
        }).max_by_key(|(version, _)| *version).map(|(_, version)| version)
        .ok_or_else(|| "No se encontró una versión estable de Spotifast con su bundle y checksum".to_string())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::other("Ruta de caché no válida"))?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
