//! Import sources into the project's portable, static mesh format.

use anyhow::{Context, Result, bail, ensure};
use std::ffi::OsStr;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;

pub const EXTENSIONS: &[&str] = &["obj", "glb", "gltf", "fbx", "usdz", "stl", "kmodel"];
pub const SOURCE_EXTENSIONS: &[&str] = &["obj", "glb", "gltf", "fbx", "usdz", "stl"];
pub const EDITABLE_EXTENSIONS: &[&str] = &["obj", "kmodel"];

pub fn is_obj(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("obj"))
}

pub fn needs_conversion(path: &Path) -> Result<bool> {
    if !is_obj(path) {
        return Ok(true);
    }
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    use std::io::BufRead as _;
    for line in std::io::BufReader::new(file).lines() {
        if line?.split_whitespace().next() == Some("mtllib") {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn validate_extension(source: &Path) -> Result<()> {
    let extension = source.extension().context("The dropped file has no model extension")?;
    ensure!(
        SOURCE_EXTENSIONS.iter().any(|ext| extension.eq_ignore_ascii_case(ext)),
        "Kit can import OBJ, GLB, glTF, FBX, USDZ, and STL model files"
    );
    Ok(())
}

pub struct ImportQueue {
    jobs: mpsc::Sender<(PathBuf, PathBuf)>,
    results: mpsc::Receiver<Result<String>>,
    pending: usize,
}

impl ImportQueue {
    pub fn new() -> Result<Self> {
        let (jobs, requests) = mpsc::channel::<(PathBuf, PathBuf)>();
        let (results, completed) = mpsc::channel();
        std::thread::Builder::new().name("kit-model-import".into()).spawn(move || {
            for (source, root) in requests {
                let result = import(&source, &root).with_context(|| format!("importing {}", source.display()));
                if results.send(result).is_err() {
                    break;
                }
            }
        })?;
        Ok(Self { jobs, results: completed, pending: 0 })
    }

    pub fn submit(&mut self, source: &Path, root: PathBuf) -> Result<()> {
        self.jobs.send((source.to_path_buf(), root)).context("the model import worker stopped")?;
        self.pending += 1;
        Ok(())
    }

    pub fn poll(&mut self) -> (Vec<Result<String>>, bool) {
        let results: Vec<_> = self.results.try_iter().collect();
        self.pending -= results.len();
        (results, self.pending > 0)
    }
}

pub fn import(source: &Path, root: &Path) -> Result<String> {
    let name = source.file_stem().and_then(OsStr::to_str).context("Model name must be valid UTF-8")?;
    let name = name.trim_start_matches('.');
    let name = if name.is_empty() { "model" } else { name };
    // Keep the original directory so the converter can resolve glTF sidecars.
    let source = std::path::absolute(source)?;
    let needs_conversion = needs_conversion(&source)?;
    if !needs_conversion {
        let canonical = source.canonicalize().with_context(|| format!("opening {}", source.display()))?;
        if canonical == source
            && source.strip_prefix(root).ok().is_some_and(|path| {
                path.iter().all(|part| {
                    let part = part.to_string_lossy();
                    !part.starts_with('.') && part != "build" && part != "target"
                })
            })
        {
            kit::render::load_obj(&source)?;
            return Ok(source.file_name().unwrap_or_default().to_string_lossy().into_owned());
        }
        kit::render::load_obj(&source)?;
    }

    let dir = root.join("models");
    match std::fs::create_dir(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir() => {}
        Err(error) => return Err(error).with_context(|| format!("creating {}", dir.display())),
    }
    ensure!(dir.canonicalize()?.starts_with(root), "The models folder must stay inside the project");
    if needs_conversion {
        let conversion = tempfile::tempdir_in(&dir).context("creating temporary model conversion directory")?;
        let manifest = convert(&source, conversion.path())?;
        let resources = kit::resource::Resources::new(conversion.path());
        let asset = kit::render::model_asset::ModelAsset::load(&resources, "geometry.kmodel")?;
        for warning in &asset.warnings {
            log::warn!("{}: {warning}", source.display());
        }
        let files = asset
            .frames
            .iter()
            .flatten()
            .flatten()
            .chain(asset.materials.iter().filter_map(|material| material.texture.as_ref()));
        for file in files {
            ensure!(conversion.path().join(file).is_file(), "the converted model is missing {file}");
        }
        std::fs::remove_file(conversion.path().join("import.py"))?;
        for suffix in 1.. {
            let stem = if suffix == 1 { name.to_owned() } else { format!("{name}-{suffix}") };
            let filename = format!("{stem}.kmodel");
            let destination = dir.join(&stem);
            match std::fs::create_dir(&destination) {
                Ok(()) => {
                    let result = (|| -> Result<()> {
                        for entry in std::fs::read_dir(conversion.path())? {
                            let entry = entry?;
                            if entry.path() != manifest {
                                std::fs::rename(entry.path(), destination.join(entry.file_name()))?;
                            }
                        }
                        std::fs::rename(manifest, destination.join(&filename))?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        let _ = std::fs::remove_dir_all(&destination);
                        return Err(error).with_context(|| format!("saving {}", destination.display()));
                    }
                    return Ok(filename);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error).with_context(|| format!("creating {}", destination.display())),
            }
        }
        unreachable!();
    }
    let mut staged = tempfile::NamedTempFile::new_in(&dir)?;
    std::io::copy(&mut std::fs::File::open(&source)?, &mut staged)?;
    staged.flush()?;
    for suffix in 1.. {
        let filename = if suffix == 1 { format!("{name}.obj") } else { format!("{name}-{suffix}.obj") };
        let destination = dir.join(&filename);
        match staged.persist_noclobber(&destination) {
            Ok(_) => return Ok(filename),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => staged = error.file,
            Err(error) => return Err(error.error).with_context(|| format!("saving {}", destination.display())),
        }
    }
    unreachable!()
}

fn convert(source: &Path, scratch: &Path) -> Result<PathBuf> {
    let script = scratch.join("import.py");
    let destination = scratch.join("geometry.kmodel");
    std::fs::write(&script, include_str!("model_import.py"))?;

    let run = |executable: &OsStr| -> std::io::Result<Output> {
        let mut command = Command::new(executable);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        command
            .args(["--background", "--factory-startup", "--disable-autoexec", "--python-exit-code", "1", "--python"])
            .arg(&script)
            .arg("--")
            .arg(source)
            .arg(&destination)
            .output()
    };

    let output = if let Some(executable) = std::env::var_os("KIT_BLENDER") {
        run(&executable).with_context(|| format!("starting Blender at {}", executable.to_string_lossy()))?
    } else {
        match run(OsStr::new("blender")).or_else(|error| {
            if error.kind() != std::io::ErrorKind::NotFound {
                return Err(error);
            }
            match installed_blender() {
                Some(executable) => run(executable.as_os_str()),
                None => Err(error),
            }
        }) {
            Ok(output)
                if cfg!(target_os = "linux")
                    && !output.status.success()
                    && String::from_utf8_lossy(&output.stderr).contains("snap-confine")
                    && Path::new("/snap/blender/current/blender").is_file() =>
            {
                run(OsStr::new("/snap/blender/current/blender")).context("starting the Blender Snap executable")?
            }
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                bail!("Install Blender 4.5 or newer for this format, or set KIT_BLENDER to its executable")
            }
            Err(error) => return Err(error).context("starting Blender"),
        }
    };

    if !output.status.success() || !destination.is_file() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = stderr
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .or_else(|| stdout.lines().rfind(|line| !matches!(line.trim(), "" | "Blender quit")));
        bail!(
            "Could not import {}. {}",
            source.display(),
            details.unwrap_or("Blender produced no mesh. Check that Blender 4.5 or newer is installed")
        );
    }
    Ok(destination)
}

fn installed_blender() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from("/Applications/Blender.app/Contents/MacOS/Blender");
        return path.is_file().then_some(path);
    }
    #[cfg(windows)]
    {
        let installs = PathBuf::from(std::env::var_os("ProgramFiles")?).join("Blender Foundation");
        return std::fs::read_dir(installs)
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let path = entry.path().join("blender.exe");
                let name = entry.file_name();
                let version: Vec<u32> = name
                    .to_str()?
                    .strip_prefix("Blender ")?
                    .split('.')
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .ok()?;
                path.is_file().then_some((version, path))
            })
            .max()
            .map(|(_, path)| path);
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    None
}
