use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::mpsc;
use std::time::Duration;
use tabled::settings::Style;
use tabled::{Table, Tabled};

const MANAGED_PROGRAMS: &[&str] = &[
    "bat",
    "bottom",
    "bpftool",
    "bpftop",
    "cargo-proxy",
    "docker-compose",
    "dust",
    "eza",
    "just",
    "kyanos",
    "motdyn",
    "sd",
    "tcping",
    "uv",
];

const UNVERSIONED_PROGRAMS: &[&str] = &[
    "bpftop",
    "kyanos,"
];

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = None,
    subcommand_required = true,
    arg_required_else_help = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Install a program
    Install(InstallArgs),
    /// List installed programs
    List,
}

#[derive(Parser, Debug)]
struct InstallArgs {
    /// The program name to install
    program: String,

    /// Output directory
    #[arg(long = "out", default_value = "/usr/local/bin")]
    out: String,

    /// Base temporary directory
    #[arg(long = "temp", default_value = "/tmp/sbin")]
    temp: String,

    /// Force installation even if the version is up-to-date
    #[arg(long = "force")]
    force: bool,
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

#[derive(Tabled)]
struct ProgramInfo {
    #[tabled(rename = "Program")]
    program: String,
    #[tabled(rename = "Version")]
    version: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install(args) => {
            install_program(&args)?;
        }
        Commands::List => {
            list_installed_programs()?;
        }
    }

    Ok(())
}

/// Install or upgrade a program
fn install_program(args: &InstallArgs) -> Result<()> {
    let program = args.program.as_str();

    // Check if the program is in the managed list
    if !MANAGED_PROGRAMS.contains(&program) {
        return Err(anyhow!(
            "Program '{}' is not managed by sbin. Managed programs are: {:?}",
            program,
            MANAGED_PROGRAMS
        ));
    }

    let binary_target_path = Path::new(&args.out).join(program);

    // Check if the binary exists and get its version
    let existing_version = if binary_target_path.exists() {
        if let Some(existing_version_output) = check_existing_version(program) {
            parse_version(&existing_version_output)
        } else {
            None
        }
    } else {
        None
    };

    // Download the image and get the new version
    let (new_version, extracted_binary_path) = download_image_and_get_version(program, &args.temp)?;

    if let Some(existing_version) = existing_version {
        println!("Detected existing version: {}", existing_version);
        println!("Available new version: {}", new_version);

        if new_version > existing_version {
            println!("A newer version is available. Do you want to upgrade? (y/n): ");
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("y") {
                // Proceed with installation
                perform_installation(&extracted_binary_path, &args.out)?;
                println!("Upgraded '{}' to version {}", program, new_version);
            } else {
                println!("Installation aborted by user.");
            }
        } else if new_version == existing_version {
            if args.force {
                println!("Force flag detected. Reinstalling '{}'.", program);
                perform_installation(&extracted_binary_path, &args.out)?;
                println!("Reinstalled '{}' version {}", program, new_version);
            } else {
                println!("No installation needed. Installed version is up-to-date.");
            }
        } else {
            println!("Installed version is newer than the available version.");
        }
    } else {
        // Binary does not exist, proceed with installation
        perform_installation(&extracted_binary_path, &args.out)?;
        println!("Installed '{}'.", program);
    }

    Ok(())
}

/// Download Docker image, extract it, and get the new version
fn download_image_and_get_version(
    program: &str,
    base_temp_dir: &str,
) -> Result<(Version, PathBuf)> {
    let registry_url = "https://registry-1.docker.io".to_string();
    let tag = "latest".to_string();
    let image_name = format!("lvillis/{}", program);

    println!("Using image: docker.io/{}:{}", image_name, tag);

    let client = Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    // Create the program-specific temporary directory
    let temp_path = Path::new(base_temp_dir).join(program);
    fs::create_dir_all(&temp_path)
        .with_context(|| format!("Failed to create temp directory {:?}", temp_path))?;

    // Create extract_path within temp_path
    let extract_path = temp_path.join("extract");
    fs::create_dir_all(&extract_path)
        .with_context(|| format!("Failed to create extract directory {:?}", extract_path))?;

    println!("Getting auth token...");
    let scope = format!("repository:{}:pull", image_name);
    let token = get_auth_token(&client, &registry_url, &image_name, &scope)?;
    println!("Got auth token.");

    println!("Getting image manifest...");
    let manifest = get_manifest(&client, &registry_url, &image_name, &tag, &token)?;
    println!("Got image manifest.");

    let layer_digests: Vec<String> = manifest
        .layers
        .iter()
        .map(|layer| layer.digest.clone())
        .collect();

    println!("Downloading and applying layers...");
    for (idx, digest) in layer_digests.iter().enumerate() {
        let layer_filename = format!("layer{}.tar.gz", idx + 1);
        let layer_path = temp_path.join(&layer_filename);
        download_blob(
            &client,
            &registry_url,
            &image_name,
            digest,
            &token,
            &layer_path,
        )?;
        apply_layer(&layer_path, &extract_path)?;
        println!("Applied layer {}", layer_filename);
    }

    println!("All layers applied successfully.");

    // Locate the binary in the extracted files
    let binary_path = extract_path
        .join("usr")
        .join("local")
        .join("bin")
        .join(program);

    if !binary_path.exists() {
        return Err(anyhow!(
            "Binary '{}' not found in the Docker image.",
            binary_path.display()
        ));
    }

    println!(
        "Running '{} --version' to get new version...",
        binary_path.display()
    );

    let output = ProcessCommand::new(&binary_path)
        .arg("--version")
        .output()
        .context("Failed to execute the new binary to get version")?;

    if !output.status.success() {
        return Err(anyhow!(
            "New binary '{}' exited with status {}",
            binary_path.display(),
            output.status
        ));
    }

    let version_output = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    println!("New version output: {}", version_output);

    // Parse the version from the output
    let new_version = parse_version(&version_output)
        .ok_or_else(|| anyhow!("Failed to parse version from output: '{}'", version_output))?;

    Ok((new_version, binary_path))
}

/// Perform the installation by copying the binary to the target directory
fn perform_installation(extracted_binary_path: &Path, target_dir: &str) -> Result<()> {
    let binary_target_path = Path::new(target_dir).join(
        extracted_binary_path
            .file_name()
            .ok_or_else(|| anyhow!("Failed to get binary file name"))?,
    );

    println!(
        "Copying binary from {:?} to {:?}",
        extracted_binary_path, binary_target_path
    );
    fs::copy(extracted_binary_path, &binary_target_path).with_context(|| {
        format!(
            "Failed to copy binary from {:?} to {:?}",
            extracted_binary_path, binary_target_path
        )
    })?;
    println!("Binary installed at {:?}", binary_target_path);

    // Ensure the binary has executable permissions on Unix systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_target_path)
            .with_context(|| format!("Failed to get permissions for {:?}", binary_target_path))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_target_path, perms).with_context(|| {
            format!(
                "Failed to set executable permissions for {:?}",
                binary_target_path
            )
        })?;
    }

    Ok(())
}

/// Get auth token from Docker registry
fn get_auth_token(
    client: &Client,
    _registry_url: &str,
    _repository: &str,
    scope: &str,
) -> Result<String> {
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
fn get_manifest(
    client: &Client,
    registry_url: &str,
    image_name: &str,
    tag: &str,
    token: &str,
) -> Result<Manifest> {
    let manifest_url = format!("{}/v2/{}/manifests/{}", registry_url, image_name, tag);
    println!("Requesting image manifest URL: {}", manifest_url);
    let resp = client
        .get(&manifest_url)
        .header(AUTHORIZATION, format!("Bearer {}", token))
        .header(
            ACCEPT,
            "application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json",
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

        let single_manifest_url = format!(
            "{}/v2/{}/manifests/{}",
            registry_url, image_name, selected_manifest.digest
        );
        println!("Requesting single manifest URL: {}", single_manifest_url);
        let single_resp = client
            .get(&single_manifest_url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .header(
                ACCEPT,
                "application/vnd.docker.distribution.manifest.v2+json, application/vnd.oci.image.manifest.v1+json",
            )
            .send()
            .context("Failed to request single manifest")?
            .error_for_status()
            .context("Error response while getting single manifest")?;

        println!("Single manifest response status: {}", single_resp.status());
        println!(
            "Single manifest response content type: {:?}",
            single_resp.headers().get(reqwest::header::CONTENT_TYPE)
        );

        let single_manifest: Manifest = single_resp
            .json()
            .context("Failed to parse single manifest")?;
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

    let content = resp
        .bytes()
        .context(format!("Failed to read blob content {}", digest))?;
    let mut file = File::create(output_path)
        .with_context(|| format!("Failed to create file {:?}", output_path))?;
    file.write_all(&content)
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
            if entry.header().entry_type().is_symlink()
                || entry.header().entry_type().is_hard_link()
            {
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

/// Check existing binary version with a timeout to prevent hanging
fn check_existing_version(program: &str) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    let program = program.to_string();

    std::thread::spawn(move || {
        let output = ProcessCommand::new(&program).arg("--version").output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout).to_string();
                    let _ = tx.send(Some(version.trim().to_string()));
                } else {
                    let _ = tx.send(None);
                }
            }
            Err(_) => {
                let _ = tx.send(None);
            }
        }
    });

    rx.recv_timeout(Duration::from_secs(2)).unwrap_or_else(|_| {
        None
    })
}

/// Parse version from version output
fn parse_version(version_output: &str) -> Option<Version> {
    // Assume version output contains a semantic version number, e.g., "just 1.38.0"
    // Extract the first valid semantic version found in the output
    for part in version_output.split_whitespace() {
        if let Ok(ver) = Version::parse(part.trim_start_matches('v')) {
            return Some(ver);
        }
    }
    None
}

/// List installed programs and their versions using tabled
fn list_installed_programs() -> Result<()> {
    let out_dir = "/usr/local/bin";

    let mut programs_info = Vec::new();

    for &program in MANAGED_PROGRAMS.iter() {
        let binary_path = Path::new(out_dir).join(program);
        if binary_path.exists() {
            if UNVERSIONED_PROGRAMS.contains(&program) {
                programs_info.push(ProgramInfo {
                    program: program.to_string(),
                    version: "Unsupported".to_string(),
                });
            } else {
                if let Some(version_output) = check_existing_version(program) {
                    if version_output.is_empty() {
                        programs_info.push(ProgramInfo {
                            program: program.to_string(),
                            version: "Unable to retrieve".to_string(),
                        });
                    } else {
                        programs_info.push(ProgramInfo {
                            program: program.to_string(),
                            version: version_output,
                        });
                    }
                } else {
                    programs_info.push(ProgramInfo {
                        program: program.to_string(),
                        version: "Unable to retrieve".to_string(),
                    });
                }
            }
        } else {
            programs_info.push(ProgramInfo {
                program: program.to_string(),
                version: "Not installed".to_string(),
            });
        }
    }

    let table = Table::new(programs_info).with(Style::rounded()).to_string();

    println!("{}", table);

    Ok(())
}
