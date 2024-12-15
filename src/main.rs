use anyhow::{anyhow, Context, Result};
use clap::Parser;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "sbin")]
#[command(about = "Fetch a specified program from Docker registry and install it to /usr/local/bin")]
struct Args {
    /// The program name to install
    program: String,

    /// Output directory (default: /usr/local/bin)
    #[arg(long="out", default_value = "/usr/local/bin")]
    out: String,

    /// Base temporary directory (default: /tmp/sbin)
    #[arg(long="temp", default_value = "/tmp/sbin")]
    temp: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct Manifest {
    layers: Vec<Layer>,
    config: Config,
}

#[derive(Deserialize, Serialize, Debug)]
struct Layer {
    digest: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct Config {
    digest: String,
}

#[derive(Deserialize, Debug)]
struct ManifestList {
    schemaVersion: u32,
    manifests: Vec<ManifestDescriptor>,
}

#[derive(Deserialize, Debug)]
struct ManifestDescriptor {
    digest: String,
    #[serde(rename = "mediaType")]
    media_type: String,
    platform: Platform,
}

#[derive(Deserialize, Debug)]
struct Platform {
    architecture: String,
    os: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let registry_url = "https://registry-1.docker.io".to_string();
    let tag = "latest".to_string();
    let image_name = format!("lvillis/{}", args.program);

    println!("Using image: docker.io/{}:{}", image_name, tag);

    let client = Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    fs::create_dir_all(&args.out)
        .with_context(|| format!("Failed to create output directory {}", args.out))?;

    // Create a unique temp directory for this program
    let temp_path = Path::new(&args.temp).join(&args.program);
    fs::create_dir_all(&temp_path)
        .with_context(|| format!("Failed to create temp directory {:?}", temp_path))?;

    println!("Getting auth token...");
    let scope = format!("repository:{}:pull", image_name);
    let token = get_auth_token(&client, &registry_url, &image_name, &scope)?;
    println!("Got auth token.");

    println!("Getting image manifest...");
    let manifest = get_manifest(&client, &registry_url, &image_name, &tag, &token)?;
    println!("Got image manifest.");

    let layer_digests: Vec<String> = manifest.layers.iter().map(|layer| layer.digest.clone()).collect();
    let config_digest = manifest.config.digest.clone();

    println!("Downloading layers...");
    for (idx, digest) in layer_digests.iter().enumerate() {
        let layer_filename = format!("layer{}.tar.gz", idx + 1);
        let layer_path = temp_path.join(&layer_filename);
        download_blob(&client, &registry_url, &image_name, digest, &token, &layer_path)?;
    }

    println!("Downloading config file...");
    let config_path = temp_path.join("config.json");
    download_blob(&client, &registry_url, &image_name, &config_digest, &token, &config_path)?;
    println!("Config file downloaded to {:?}", config_path);

    println!("Applying layers...");
    for idx in 1..=layer_digests.len() {
        let layer_filename = format!("layer{}.tar.gz", idx);
        let layer_path = temp_path.join(&layer_filename);
        apply_layer(&layer_path, &temp_path)?;
        println!("Applied layer {}", layer_filename);
    }

    println!("All layers applied successfully.");

    let binary_source_path = temp_path.join("usr").join("local").join("bin").join(&args.program);
    let binary_target_path = Path::new(&args.out).join(&args.program);

    println!("Copying binary from {:?} to {:?}", binary_source_path, binary_target_path);
    fs::copy(&binary_source_path, &binary_target_path).with_context(|| {
        format!(
            "Failed to copy binary from {:?} to {:?}",
            binary_source_path, binary_target_path
        )
    })?;
    println!("Binary installed at {:?}", binary_target_path);

    Ok(())
}

/// Get auth token from Docker registry
fn get_auth_token(client: &Client, _registry_url: &str, _repository: &str, scope: &str) -> Result<String> {
    let auth_url = "https://auth.docker.io/token";
    println!("Requesting auth token URL: {}", auth_url);
    let resp = client
        .get(auth_url)
        .query(&[("service", "registry.docker.io"), ("scope", scope)])
        .send()
        .context("Failed to request auth token")?
        .error_for_status()
        .context("Error response while getting auth token")?;

    println!("Auth token response status: {}", resp.status());
    println!(
        "Auth token response content type: {:?}",
        resp.headers().get(reqwest::header::CONTENT_TYPE)
    );

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_resp: TokenResponse = resp.json().context("Failed to parse auth token response")?;
    Ok(token_resp.access_token)
}

/// Get image manifest from Docker registry
fn get_manifest(client: &Client, registry_url: &str, image_name: &str, tag: &str, token: &str) -> Result<Manifest> {
    let manifest_url = format!("{}/v2/{}/manifests/{}", registry_url, image_name, tag);
    println!("Requesting image manifest URL: {}", manifest_url);
    let resp = client
        .get(&manifest_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .header(
            ACCEPT,
            "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json"
        )
        .send()
        .context("Failed to request image manifest")?
        .error_for_status()
        .context("Error response while getting image manifest")?;

    println!("Image manifest response status: {}", resp.status());
    println!(
        "Image manifest response content type: {:?}",
        resp.headers().get(reqwest::header::CONTENT_TYPE)
    );

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .unwrap_or("");

    if content_type.contains("image.index") || content_type.contains("manifest.list") {
        println!("Detected manifest list or OCI image index, parsing...");
        let manifest_list: ManifestList = resp.json().context("Failed to parse manifest list")?;

        let desired_platform = ("linux", "amd64");
        let selected_manifest = manifest_list.manifests.iter().find(|m| {
            m.platform.os == desired_platform.0 && m.platform.architecture == desired_platform.1
        });

        let selected_manifest = selected_manifest.ok_or_else(|| {
            anyhow!(
                "No suitable platform found for {} / {}",
                desired_platform.0,
                desired_platform.1
            )
        })?;

        println!(
            "Selected manifest digest: {} for platform {}/{}",
            selected_manifest.digest,
            selected_manifest.platform.os,
            selected_manifest.platform.architecture
        );

        let single_manifest_url = format!("{}/v2/{}/manifests/{}", registry_url, image_name, selected_manifest.digest);
        println!("Requesting single manifest URL: {}", single_manifest_url);
        let single_resp = client
            .get(&single_manifest_url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .header(ACCEPT, "application/vnd.docker.distribution.manifest.v2+json, application/vnd.oci.image.manifest.v1+json")
            .send()
            .context("Failed to request single manifest")?
            .error_for_status()
            .context("Error response while getting single manifest")?;

        println!("Single manifest response status: {}", single_resp.status());
        println!(
            "Single manifest response content type: {:?}",
            single_resp.headers().get(reqwest::header::CONTENT_TYPE)
        );

        let single_manifest: Manifest = single_resp.json().context("Failed to parse single manifest")?;
        Ok(single_manifest)
    } else if content_type.contains("manifest.v2") || content_type.contains("image.manifest") {
        println!("Detected single manifest, parsing...");
        let manifest: Manifest = resp.json().context("Failed to parse single manifest")?;
        Ok(manifest)
    } else {
        Err(anyhow!("Unsupported manifest type: {}", content_type))
    }
}

/// Download blob (layer or config)
fn download_blob(
    client: &Client,
    registry_url: &str,
    image_name: &str,
    digest: &str,
    token: &str,
    output_path: &Path,
) -> Result<()> {
    let blob_url = format!("{}/v2/{}/blobs/{}", registry_url, image_name, digest);
    println!("Requesting blob URL: {}", blob_url);
    let resp = client
        .get(&blob_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .send()
        .context(format!("Failed to request blob {}", digest))?
        .error_for_status()
        .context(format!("Error response while getting blob {}", digest))?;

    println!("Blob response status: {}", resp.status());
    println!(
        "Blob response content type: {:?}",
        resp.headers().get(reqwest::header::CONTENT_TYPE)
    );

    let content = resp.bytes().context(format!("Failed to read blob content {}", digest))?;
    let mut file = File::create(output_path)
        .with_context(|| format!("Failed to create file {:?}", output_path))?;
    io::copy(&mut content.as_ref(), &mut file)
        .context(format!("Failed to write blob {} to file", digest))?;
    println!("Downloaded: {:?}", output_path);
    Ok(())
}

/// Apply image layer to filesystem
fn apply_layer(layer_tar_path: &Path, fs_dir: &Path) -> Result<()> {
    println!("Applying layer: {:?}", layer_tar_path);
    let tar_gz = File::open(layer_tar_path)
        .with_context(|| format!("Cannot open file {:?}", layer_tar_path))?;
    let decompressor = GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(decompressor);

    for entry_result in archive.entries().context("Failed to read tar entries")? {
        let mut entry = entry_result.context("Failed to read tar entry")?;
        let path = entry.path()?.to_owned();

        if cfg!(windows) {
            if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
                println!("Skipping link: {:?}", path);
                continue;
            }
        }

        let path_str = path.to_string_lossy().into_owned();

        if let Err(e) = entry.unpack_in(fs_dir) {
            return Err(anyhow!("Cannot unpack entry {:?}: {}", path_str, e));
        }
    }

    Ok(())
}
