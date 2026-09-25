//! The serve command line: its grammar, and what a bind beyond this machine exposes.

#[derive(Debug)]
pub struct Cli {
    /// `None` until `--port` names one; [`DEFAULT_PORT`] otherwise.
    pub port: Option<u16>,
    pub bind: String,
    /// Node source roots scanned before the patch's own; a later entry wins a shared type name.
    pub extra_nodes: Vec<String>,
    pub list_nodes: bool,
    /// Serve the API alone: the SPA's routes are never mounted. Also set by `GOOFI_HEADLESS` in
    /// the environment and by a binary built with it, both folded in by the binary.
    pub headless: bool,
    /// Open `/dev/*`, the development surfaces. Also set by `GOOFI_DEBUG` in the environment.
    pub debug: bool,
    /// A PUBLIC goofi: no terminal, no agents, no filesystem, no save or load, no audio. Also set
    /// by `GOOFI_DEMO` in the environment. Not a sandbox.
    pub demo: bool,
    /// A patch to open before the first client connects. Also `GOOFI_LOAD` in the environment.
    pub load: Option<String>,
    pub help: bool,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            port: None,
            bind: String::from("127.0.0.1"),
            extra_nodes: Vec::new(),
            list_nodes: false,
            headless: false,
            debug: false,
            demo: false,
            load: None,
            help: false,
        }
    }
}

pub const USAGE: &str = "usage: goofi [serve] [--port N] [--bind HOST] \
     [--extra-nodes DIR] [--list-nodes] [--headless] [--debug] [--demo] [--load PATCH]";

/// The port with no door naming one.
pub const DEFAULT_PORT: u16 = 8000;

/// Parse the argument list (already skipping argv[0]). `Err` is the message to print before
/// exiting 2.
pub fn parse_args<I: Iterator<Item = String>>(mut args: I) -> Result<Cli, String> {
    let mut cli = Cli::default();
    while let Some(arg) = args.next() {
        let need = |v: Option<String>| v.ok_or_else(|| format!("{arg} requires a value (try --help)"));
        match arg.as_str() {
            "--port" => {
                let v = need(args.next())?;
                cli.port = Some(v.parse().map_err(|_| format!("invalid --port `{v}`"))?);
            }
            "--bind" => cli.bind = need(args.next())?,
            "--extra-nodes" => cli.extra_nodes.push(need(args.next())?),
            "--list-nodes" => cli.list_nodes = true,
            "--headless" => cli.headless = true,
            "--debug" => cli.debug = true,
            "--demo" => cli.demo = true,
            "--load" => cli.load = Some(need(args.next())?),
            "-h" | "--help" => cli.help = true,
            other => return Err(format!("unknown argument `{other}` (try --help)")),
        }
    }
    Ok(cli)
}

/// The warning a `--bind` beyond this machine earns, or `None` for the loopback default. A name
/// that is not an address warns too: only a parseable address can be proven local.
pub fn exposure_warning(bind: &str) -> Option<String> {
    let local = bind == "localhost"
        || bind.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    (!local).then(|| {
        format!(
            "WARNING: --bind {bind} serves goofi beyond this machine, and goofi runs agent \
             harnesses on a shell with your environment. Anyone who can reach this port can run \
             commands as you: there is no authentication, only a guard against a web page \
             reaching it through your browser."
        )
    })
}
