//! Known-service detection.
//!
//! Detection runs in three passes, most reliable first:
//! 1. **Command line** — for generic runtimes (`node`, `python`, `java`, …) the
//!    arguments name the framework (`next dev`, `manage.py runserver`, `kafka.Kafka`).
//! 2. **Process name** — daemons usually name themselves (`postgres`, `redis-server`).
//! 3. **Port** — a well-known port is only a guess ([`ServiceSource::Port`]); the
//!    UI shows it with a trailing `?`.

use crate::models::{Service, ServiceCategory as C, ServiceSource};

/// How to match a process name
#[derive(Clone, Copy)]
enum Match {
    /// name.contains(pattern)
    Contains(&'static str),
    /// name == pattern
    Exact(&'static str),
    /// name.starts_with(pattern)
    Prefix(&'static str),
    /// name.contains(a) && name.contains(b)
    Both(&'static str, &'static str),
}

/// How to match a command line. Arguments are compared as lowercase basenames
/// without a script extension (`/app/node_modules/.bin/next` → `next`,
/// `manage.py` → `manage`).
#[derive(Clone, Copy)]
enum Cmd {
    /// An argument is this program — by basename, or as a path inside
    /// `node_modules/<name>/` (npm bins are often realpath'd, e.g.
    /// `node_modules/vite/bin/vite.js`; scoped packages like `@angular/cli` work too).
    Bin(&'static str),
    /// Argument `.0` followed later by argument `.1` (`manage` … `runserver`).
    Then(&'static str, &'static str),
    /// The lowercase command line contains this text (JVM main classes, titles).
    Text(&'static str),
}

/// Command-line rules, tried first and only for generic runtimes (see [`is_runtime`]).
/// The first match wins.
const COMMAND_RULES: &[(Cmd, &str, C)] = &[
    // JVM services — the process is just `java`
    (Cmd::Text("kafka.kafka"), "Kafka", C::Queue),
    (
        Cmd::Text("org.elasticsearch.bootstrap.elasticsearch"),
        "Elasticsearch",
        C::Database,
    ),
    (
        Cmd::Text("org.opensearch.bootstrap.opensearch"),
        "OpenSearch",
        C::Database,
    ),
    (Cmd::Text("org.apache.cassandra"), "Cassandra", C::Database),
    (Cmd::Text("org.neo4j"), "Neo4j", C::Database),
    (Cmd::Text("org.apache.solr"), "Solr", C::Database),
    (Cmd::Text("org.apache.zookeeper"), "ZooKeeper", C::System),
    (Cmd::Text("org.apache.catalina"), "Tomcat", C::System),
    (Cmd::Text("org.keycloak"), "Keycloak", C::System),
    (Cmd::Text("org.sonar"), "SonarQube", C::System),
    (Cmd::Text("jenkins.war"), "Jenkins", C::System),
    // Erlang VM
    (Cmd::Text("phx.server"), "Phoenix", C::DevServer),
    (Cmd::Text("rabbit"), "RabbitMQ", C::Queue),
    // JavaScript dev servers
    (Cmd::Text("next-server"), "Next.js", C::DevServer),
    (Cmd::Bin("next"), "Next.js", C::DevServer),
    (Cmd::Bin("vite"), "Vite", C::DevServer),
    (Cmd::Bin("astro"), "Astro", C::DevServer),
    (Cmd::Bin("nuxi"), "Nuxt", C::DevServer),
    (Cmd::Bin("nuxt"), "Nuxt", C::DevServer),
    (Cmd::Bin("@remix-run/dev"), "Remix", C::DevServer),
    (Cmd::Bin("gatsby"), "Gatsby", C::DevServer),
    (Cmd::Bin("@docusaurus/core"), "Docusaurus", C::DevServer),
    (Cmd::Bin("storybook"), "Storybook", C::DevServer),
    (Cmd::Bin("@storybook/cli"), "Storybook", C::DevServer),
    (Cmd::Bin("react-scripts"), "Create React App", C::DevServer),
    (Cmd::Bin("webpack-dev-server"), "Webpack", C::DevServer),
    (Cmd::Then("webpack", "serve"), "Webpack", C::DevServer),
    (Cmd::Bin("@angular/cli"), "Angular", C::DevServer),
    (Cmd::Then("ng", "serve"), "Angular", C::DevServer),
    (Cmd::Bin("@expo/cli"), "Expo", C::DevServer),
    (Cmd::Bin("expo"), "Expo", C::DevServer),
    (
        Cmd::Bin("@react-native-community/cli"),
        "Metro",
        C::DevServer,
    ),
    (Cmd::Bin("metro"), "Metro", C::DevServer),
    (Cmd::Bin("wrangler"), "Wrangler", C::DevServer),
    (
        Cmd::Bin("firebase-tools"),
        "Firebase Emulators",
        C::DevServer,
    ),
    (Cmd::Bin("json-server"), "json-server", C::DevServer),
    (Cmd::Bin("http-server"), "http-server", C::DevServer),
    (Cmd::Bin("live-server"), "live-server", C::DevServer),
    (Cmd::Bin("code-server"), "code-server", C::DevServer),
    // Python
    (Cmd::Bin("uvicorn"), "Uvicorn", C::DevServer),
    (Cmd::Bin("gunicorn"), "Gunicorn", C::DevServer),
    (Cmd::Bin("hypercorn"), "Hypercorn", C::DevServer),
    (Cmd::Bin("daphne"), "Daphne", C::DevServer),
    (Cmd::Then("manage", "runserver"), "Django", C::DevServer),
    (Cmd::Then("fastapi", "dev"), "FastAPI", C::DevServer),
    (Cmd::Then("fastapi", "run"), "FastAPI", C::DevServer),
    (Cmd::Bin("flask"), "Flask", C::DevServer),
    (Cmd::Bin("streamlit"), "Streamlit", C::DevServer),
    (Cmd::Bin("jupyter-lab"), "Jupyter", C::DevServer),
    (Cmd::Bin("jupyter-notebook"), "Jupyter", C::DevServer),
    (Cmd::Bin("jupyter-server"), "Jupyter", C::DevServer),
    (Cmd::Bin("jupyter"), "Jupyter", C::DevServer),
    (Cmd::Then("mkdocs", "serve"), "MkDocs", C::DevServer),
    (Cmd::Bin("mlflow"), "MLflow", C::DevServer),
    (Cmd::Bin("tensorboard"), "TensorBoard", C::DevServer),
    (
        Cmd::Text("-m http.server"),
        "Python http.server",
        C::DevServer,
    ),
    // Ruby / PHP
    (Cmd::Text("puma "), "Puma", C::DevServer),
    (Cmd::Then("rails", "server"), "Rails", C::DevServer),
    (Cmd::Then("rails", "s"), "Rails", C::DevServer),
    (Cmd::Then("jekyll", "serve"), "Jekyll", C::DevServer),
    (Cmd::Bin("rackup"), "Rack", C::DevServer),
    (Cmd::Then("artisan", "serve"), "Laravel", C::DevServer),
    (Cmd::Then("php", "-s"), "PHP built-in server", C::DevServer),
];

/// Process name → (service, category) mapping table.
///
/// # Matching algorithm
///
/// Each entry is `(Match, service_name, ServiceCategory)`.  The process name
/// passed in has already been lowercased by `identify`, so all patterns here
/// must also be lowercase.
///
/// Match variants:
/// - `Contains(p)` — `name.contains(p)`
/// - `Exact(p)` — `name == p`
/// - `Prefix(p)` — `name.starts_with(p)` (include the trailing space to avoid
///   false positives, e.g. `"bun "` won't match `"bundle"`)
/// - `Both(a, b)` — `name.contains(a) && name.contains(b)` — use when one
///   fragment alone is too ambiguous (e.g. `"cursor"+"helper"`)
///
/// # Priority
///
/// Command-line rules run first (for generic runtimes), then these, then
/// `PORT_RULES`. Within this table the **first** matching rule wins; put
/// more-specific patterns above less-specific ones (e.g. `"redis-server"`
/// before the bare `"redis"` exact match).
///
/// # How to add a new service
///
/// 1. If the process is a generic runtime (`node`, `python`, `java`, …), add a
///    `COMMAND_RULES` entry; otherwise choose the tightest `Match` here.
/// 2. Pick or create a `ServiceCategory` in `models.rs`.
/// 3. Add a matching port entry to `PORT_RULES` below if there is a well-known
///    default port for the service.
/// 4. Add a unit test at the bottom of this file.
const PROCESS_RULES: &[(Match, &str, C)] = &[
    // Databases
    (Match::Contains("postgres"), "PostgreSQL", C::Database),
    (Match::Contains("postmaster"), "PostgreSQL", C::Database),
    (Match::Contains("mysql"), "MySQL", C::Database),
    (Match::Contains("mariadbd"), "MySQL", C::Database),
    (Match::Contains("mariadb"), "MySQL", C::Database),
    (Match::Contains("mongod"), "MongoDB", C::Database),
    (Match::Contains("mongos"), "MongoDB", C::Database),
    (Match::Contains("clickhouse"), "ClickHouse", C::Database),
    (
        Match::Contains("elasticsearch"),
        "Elasticsearch",
        C::Database,
    ),
    (Match::Contains("opensearch"), "OpenSearch", C::Database),
    (Match::Contains("minio"), "MinIO", C::Database),
    (Match::Contains("cockroach"), "CockroachDB", C::Database),
    (Match::Contains("influxd"), "InfluxDB", C::Database),
    (Match::Contains("meilisearch"), "Meilisearch", C::Database),
    (Match::Contains("typesense"), "Typesense", C::Database),
    (Match::Contains("qdrant"), "Qdrant", C::Database),
    // Caches
    (Match::Contains("redis-server"), "Redis", C::Cache),
    (Match::Exact("redis"), "Redis", C::Cache),
    (Match::Contains("valkey-server"), "Valkey", C::Cache),
    (Match::Contains("keydb-server"), "KeyDB", C::Cache),
    (Match::Contains("dragonfly"), "Dragonfly", C::Cache),
    (Match::Contains("memcached"), "Memcached", C::Cache),
    // Message brokers
    (Match::Contains("kafka"), "Kafka", C::Queue),
    (Match::Contains("rabbitmq"), "RabbitMQ", C::Queue),
    (Match::Contains("beam.smp"), "RabbitMQ", C::Queue),
    (Match::Contains("nats-server"), "NATS", C::Queue),
    (Match::Exact("nats"), "NATS", C::Queue),
    // Containers
    (Match::Contains("docker"), "Docker", C::Container),
    (Match::Contains("containerd"), "Docker", C::Container),
    (Match::Contains("orbstack"), "OrbStack", C::Container),
    (Match::Contains("colima"), "Colima", C::Container),
    (Match::Exact("limactl"), "Lima", C::Container),
    (Match::Exact("gvproxy"), "Podman", C::Container),
    (Match::Contains("rootlessport"), "Podman", C::Container),
    (Match::Contains("kubectl"), "Kubernetes", C::Container),
    (Match::Contains("kubelet"), "Kubernetes", C::Container),
    // Dev tools / runtimes
    (Match::Contains("next-server"), "Next.js", C::DevServer),
    (Match::Contains("gunicorn"), "Gunicorn", C::DevServer),
    (Match::Prefix("puma"), "Puma", C::DevServer),
    (Match::Exact("bun"), "Bun", C::DevServer),
    (Match::Prefix("bun "), "Bun", C::DevServer),
    (Match::Exact("deno"), "Deno", C::DevServer),
    (Match::Prefix("deno "), "Deno", C::DevServer),
    (Match::Exact("hugo"), "Hugo", C::DevServer),
    (Match::Exact("zola"), "Zola", C::DevServer),
    (Match::Exact("mdbook"), "mdBook", C::DevServer),
    (Match::Exact("trunk"), "Trunk", C::DevServer),
    (Match::Contains("php-fpm"), "PHP-FPM", C::DevServer),
    (Match::Exact("mailpit"), "Mailpit", C::DevServer),
    (Match::Exact("mailhog"), "MailHog", C::DevServer),
    (Match::Contains("ollama"), "Ollama", C::DevServer),
    (Match::Contains("lm studio"), "LM Studio", C::DevServer),
    (Match::Contains("supabase"), "Supabase", C::DevServer),
    (Match::Contains("code helper"), "VS Code", C::DevServer),
    (Match::Prefix("code - "), "VS Code", C::DevServer),
    (Match::Both("cursor", "helper"), "Cursor", C::DevServer),
    (Match::Contains("electron"), "Electron", C::DevServer),
    // Browsers (macOS helper names first, then Linux binary names)
    (Match::Contains("brave browser"), "Brave", C::Browser),
    (Match::Contains("brave helper"), "Brave", C::Browser),
    (Match::Contains("google chrome"), "Chrome", C::Browser),
    (Match::Contains("chrome helper"), "Chrome", C::Browser),
    (Match::Contains("microsoft edge"), "Edge", C::Browser),
    (Match::Contains("vivaldi"), "Vivaldi", C::Browser),
    (Match::Exact("chrome"), "Chrome", C::Browser),
    (Match::Prefix("chromium"), "Chromium", C::Browser),
    (Match::Exact("brave"), "Brave", C::Browser),
    (Match::Prefix("brave-browser"), "Brave", C::Browser),
    (Match::Exact("msedge"), "Edge", C::Browser),
    (Match::Prefix("opera"), "Opera", C::Browser),
    (Match::Contains("firefox"), "Firefox", C::Browser),
    (Match::Contains("geckodriver"), "Firefox", C::Browser),
    (Match::Contains("safari"), "Safari", C::Browser),
    (Match::Contains("webkit"), "Safari", C::Browser),
    // System / infrastructure
    (Match::Contains("sshd"), "SSH", C::System),
    (Match::Contains("nginx"), "Nginx", C::System),
    (Match::Contains("lighttpd"), "lighttpd", C::System),
    (Match::Contains("httpd"), "Apache", C::System),
    (Match::Contains("apache"), "Apache", C::System),
    (Match::Contains("controlcenter"), "AirPlay", C::System),
    (Match::Contains("sharingd"), "Sharing", C::System),
    (Match::Contains("rapportd"), "Rapport", C::System),
    (Match::Contains("identityservicesd"), "Identity", C::System),
    (Match::Contains("mdnsresponder"), "mDNS", C::System),
    (Match::Contains("cupsd"), "CUPS", C::System),
    (Match::Contains("grafana"), "Grafana", C::System),
    (Match::Contains("prometheus"), "Prometheus", C::System),
    (Match::Contains("node_exporter"), "node_exporter", C::System),
    (Match::Contains("caddy"), "Caddy", C::System),
    (Match::Contains("traefik"), "Traefik", C::System),
    (Match::Contains("envoy"), "Envoy", C::System),
    (Match::Contains("haproxy"), "HAProxy", C::System),
    (Match::Contains("etcd"), "etcd", C::System),
    (Match::Contains("consul"), "Consul", C::System),
    (Match::Exact("vault"), "Vault", C::System),
];

/// Port → (service, category) mapping table. Matches here are guesses.
const PORT_RULES: &[(u16, &str, C)] = &[
    // Dev servers
    (1234, "LM Studio", C::DevServer),
    (1313, "Hugo", C::DevServer),
    (3000, "Next.js / Rails", C::DevServer),
    (3001, "React Dev", C::DevServer),
    (4000, "Phoenix", C::DevServer),
    (4200, "Angular", C::DevServer),
    (4321, "Astro", C::DevServer),
    (4566, "LocalStack", C::DevServer),
    (5000, "Flask", C::DevServer),
    (5173, "Vite", C::DevServer),
    (5174, "Vite", C::DevServer),
    (6006, "Storybook", C::DevServer),
    (7860, "Gradio", C::DevServer),
    (8000, "Django / FastAPI", C::DevServer),
    (8025, "Mailpit", C::DevServer),
    (1025, "Mailpit SMTP", C::DevServer),
    (8080, "HTTP Alt", C::DevServer),
    (8081, "Metro", C::DevServer),
    (8443, "HTTPS Alt", C::DevServer),
    (8501, "Streamlit", C::DevServer),
    (8787, "Wrangler", C::DevServer),
    (8888, "Jupyter", C::DevServer),
    (9000, "PHP-FPM", C::DevServer),
    (9229, "Node inspector", C::DevServer),
    (11434, "Ollama", C::DevServer),
    (19006, "Expo", C::DevServer),
    (54321, "Supabase API", C::DevServer),
    (54323, "Supabase Studio", C::DevServer),
    // Databases
    (1433, "SQL Server", C::Database),
    (1521, "Oracle", C::Database),
    (3306, "MySQL", C::Database),
    (5432, "PostgreSQL", C::Database),
    (5433, "PostgreSQL Alt", C::Database),
    (6333, "Qdrant", C::Database),
    (7474, "Neo4j", C::Database),
    (7687, "Neo4j", C::Database),
    (7700, "Meilisearch", C::Database),
    (8086, "InfluxDB", C::Database),
    (8123, "ClickHouse", C::Database),
    (9001, "MinIO Console", C::Database),
    (9004, "ClickHouse", C::Database),
    (9042, "Cassandra", C::Database),
    (9200, "Elasticsearch", C::Database),
    (26257, "CockroachDB", C::Database),
    (27017, "MongoDB", C::Database),
    (54322, "Supabase DB", C::Database),
    // Caches
    (6379, "Redis", C::Cache),
    (11211, "Memcached", C::Cache),
    // Message brokers
    (4222, "NATS", C::Queue),
    (5672, "RabbitMQ", C::Queue),
    (9092, "Kafka", C::Queue),
    (15672, "RabbitMQ UI", C::Queue),
    // Container / orchestration
    (2375, "Docker", C::Container),
    (2376, "Docker", C::Container),
    (6443, "Kubernetes API", C::Container),
    // System / observability
    (22, "SSH", C::System),
    (53, "DNS", C::System),
    (80, "HTTP", C::System),
    (443, "HTTPS", C::System),
    (631, "CUPS", C::System),
    (2019, "Caddy Admin", C::System),
    (2181, "ZooKeeper", C::System),
    (3100, "Loki", C::System),
    (4317, "OTLP gRPC", C::System),
    (4318, "OTLP HTTP", C::System),
    (5353, "mDNS", C::System),
    (5601, "Kibana", C::System),
    (8200, "Vault", C::System),
    (8500, "Consul", C::System),
    (9090, "Prometheus", C::System),
    (9100, "node_exporter", C::System),
    (9411, "Zipkin", C::System),
    (16686, "Jaeger", C::System),
];

/// Ports that usually speak HTTP, whatever owns them (used for "open in browser" and probes).
const WEB_PORTS: &[u16] = &[
    80, 443, 1313, 3000, 3001, 4000, 4200, 4321, 5000, 5173, 5174, 5601, 6006, 6333, 7700, 7860,
    8000, 8001, 8008, 8025, 8080, 8081, 8123, 8443, 8501, 8787, 8888, 9000, 9001, 9090, 9200,
    11434, 15672, 16686, 19006, 54321, 54323,
];

const AIRPLAY_NOTE: &str = "macOS AirPlay Receiver (ports 5000/7000). It restarts if killed — turn it off in System Settings → General → AirDrop & Handoff → AirPlay Receiver.";
const CONTAINER_PROXY_NOTE: &str = "Container runtime port forwarder — stopping it stops every container. Stop the container instead (docker stop <name>).";

fn matches_rule(name: &str, m: Match) -> bool {
    match m {
        Match::Contains(p) => name.contains(p),
        Match::Exact(p) => name == p,
        Match::Prefix(p) => name.starts_with(p),
        Match::Both(a, b) => name.contains(a) && name.contains(b),
    }
}

/// Interpreters and launchers whose arguments, not their name, identify the service.
fn is_runtime(name_lower: &str) -> bool {
    const RUNTIMES: &[&str] = &[
        "node", "nodejs", "bun", "deno", "ruby", "java", "php", "perl", "dotnet", "beam.smp",
        "erl", "uv", "npx", "npm", "pnpm", "yarn",
    ];
    RUNTIMES.contains(&name_lower) || name_lower.starts_with("python")
}

/// Lowercase basename of an argument without a script extension.
fn token(arg: &str) -> String {
    let base = arg.rsplit(['/', '\\']).next().unwrap_or(arg).to_lowercase();
    for ext in [".js", ".mjs", ".cjs", ".ts", ".py", ".rb", ".php"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

impl Cmd {
    fn matches(self, tokens: &[String], args_lower: &[String], joined: &str) -> bool {
        match self {
            Cmd::Bin(name) => {
                let in_modules = format!("node_modules/{name}/");
                tokens.iter().any(|t| t == name)
                    || args_lower
                        .iter()
                        .any(|a| a.replace('\\', "/").contains(&in_modules))
            }
            Cmd::Then(first, second) => tokens
                .iter()
                .position(|t| t == first)
                .is_some_and(|i| tokens[i + 1..].iter().any(|t| t == second)),
            Cmd::Text(text) => joined.contains(text),
        }
    }
}

/// True for processes that forward container ports (killing them stops every container).
pub fn is_container_proxy(name: &str, cmd: &[String]) -> bool {
    const PROXIES: &[&str] = &[
        "com.docker.backend",
        "com.docker.vpnkit",
        "vpnkit",
        "docker-proxy",
        "limactl",
        "gvproxy",
        "rootlessport",
        "rootlesskit",
    ];
    let n = name.to_lowercase();
    PROXIES.iter().any(|p| n.starts_with(p))
        || n.contains("orbstack")
        || (n == "ssh" && cmd.iter().any(|a| a.contains("lima")))
}

/// Ports conventionally served over TLS.
pub fn is_https_port(port: u16) -> bool {
    matches!(port, 443 | 4443 | 5001 | 8443 | 9443)
}

/// Whether a listener probably speaks HTTP (so "open in browser" makes sense).
pub fn is_http_like(port: u16, service: Option<&Service>) -> bool {
    WEB_PORTS.contains(&port) || service.is_some_and(|s| s.category == C::DevServer)
}

fn note_for(service: &str, name: &str, cmd: &[String]) -> Option<&'static str> {
    if is_container_proxy(name, cmd) {
        Some(CONTAINER_PROXY_NOTE)
    } else if service == "AirPlay" {
        Some(AIRPLAY_NOTE)
    } else {
        None
    }
}

/// Identify a known service from the command line, process name and port.
pub fn identify(port: u16, process_name: &str, cmd: &[String]) -> Option<Service> {
    let name_lower = process_name.to_lowercase();
    let make = |service: &str, category, source| Service {
        name: service.to_string(),
        category,
        source,
        note: note_for(service, process_name, cmd),
    };

    if is_runtime(&name_lower) {
        let tokens: Vec<String> = cmd.iter().map(|a| token(a)).collect();
        let args_lower: Vec<String> = cmd.iter().map(|a| a.to_lowercase()).collect();
        let joined = args_lower.join(" ");
        if let Some(&(_, service, category)) = COMMAND_RULES
            .iter()
            .find(|(rule, _, _)| rule.matches(&tokens, &args_lower, &joined))
        {
            return Some(make(service, category, ServiceSource::Command));
        }
    }

    if let Some(&(_, service, category)) = PROCESS_RULES
        .iter()
        .find(|&&(m, _, _)| matches_rule(&name_lower, m))
    {
        return Some(make(service, category, ServiceSource::Process));
    }

    PORT_RULES
        .iter()
        .find(|&&(p, _, _)| p == port)
        .map(|&(_, service, category)| make(service, category, ServiceSource::Port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ServiceCategory;

    fn identify_service(port: u16, name: &str) -> (Option<String>, ServiceCategory) {
        match identify(port, name, &[]) {
            Some(s) => (Some(s.name), s.category),
            None => (None, ServiceCategory::Unknown),
        }
    }

    #[test]
    fn port_only_match_is_marked_as_guess() {
        let s = identify(8000, "Python", &[]).unwrap();
        assert_eq!(s.source, ServiceSource::Port);
        let s = identify(9999, "postgres", &[]).unwrap();
        assert_eq!(s.source, ServiceSource::Process);
    }

    // --- Process name detection (databases) ---

    #[test]
    fn identifies_postgres_by_name() {
        let (svc, cat) = identify_service(9999, "postgres");
        assert_eq!(svc.as_deref(), Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_postmaster() {
        let (svc, cat) = identify_service(9999, "postmaster");
        assert_eq!(svc.as_deref(), Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_mysql_by_name() {
        let (svc, cat) = identify_service(9999, "mysqld");
        assert_eq!(svc.as_deref(), Some("MySQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_mariadb_by_name() {
        let (svc, cat) = identify_service(9999, "mariadbd");
        assert_eq!(svc.as_deref(), Some("MySQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_mongodb_by_name() {
        let (svc, cat) = identify_service(9999, "mongod");
        assert_eq!(svc.as_deref(), Some("MongoDB"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    // --- Process name detection (cache) ---

    #[test]
    fn identifies_redis_server() {
        let (svc, cat) = identify_service(9999, "redis-server");
        assert_eq!(svc.as_deref(), Some("Redis"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_memcached() {
        let (svc, cat) = identify_service(9999, "memcached");
        assert_eq!(svc.as_deref(), Some("Memcached"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    // --- Process name detection (containers) ---

    #[test]
    fn identifies_docker() {
        let (svc, cat) = identify_service(9999, "com.docker.backend");
        assert_eq!(svc.as_deref(), Some("Docker"));
        assert_eq!(cat, ServiceCategory::Container);
    }

    #[test]
    fn identifies_colima() {
        let (svc, cat) = identify_service(9999, "colima");
        assert_eq!(svc.as_deref(), Some("Colima"));
        assert_eq!(cat, ServiceCategory::Container);
    }

    // --- Process name detection (dev tools) ---

    #[test]
    fn identifies_vscode() {
        let (svc, cat) = identify_service(9999, "Code Helper");
        assert_eq!(svc.as_deref(), Some("VS Code"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_cursor() {
        let (svc, cat) = identify_service(9999, "Cursor Helper");
        assert_eq!(svc.as_deref(), Some("Cursor"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_ollama() {
        let (svc, cat) = identify_service(9999, "ollama");
        assert_eq!(svc.as_deref(), Some("Ollama"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    // --- Process name detection (browsers) ---

    #[test]
    fn identifies_chrome() {
        let (svc, cat) = identify_service(9999, "Google Chrome Helper");
        assert_eq!(svc.as_deref(), Some("Chrome"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    #[test]
    fn identifies_brave() {
        let (svc, cat) = identify_service(9999, "Brave Browser Helper");
        assert_eq!(svc.as_deref(), Some("Brave"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    #[test]
    fn identifies_firefox() {
        let (svc, cat) = identify_service(9999, "firefox");
        assert_eq!(svc.as_deref(), Some("Firefox"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    #[test]
    fn identifies_safari() {
        let (svc, cat) = identify_service(9999, "Safari");
        assert_eq!(svc.as_deref(), Some("Safari"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    // --- Process name detection (system) ---

    #[test]
    fn identifies_nginx() {
        let (svc, cat) = identify_service(9999, "nginx");
        assert_eq!(svc.as_deref(), Some("Nginx"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_sshd() {
        let (svc, cat) = identify_service(9999, "sshd");
        assert_eq!(svc.as_deref(), Some("SSH"));
        assert_eq!(cat, ServiceCategory::System);
    }

    // --- Port-based detection ---

    #[test]
    fn identifies_port_3000() {
        let (svc, cat) = identify_service(3000, "node");
        assert_eq!(svc.as_deref(), Some("Next.js / Rails"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_port_5432() {
        let (svc, cat) = identify_service(5432, "unknown_process");
        assert_eq!(svc.as_deref(), Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_port_6379() {
        let (svc, cat) = identify_service(6379, "unknown_process");
        assert_eq!(svc.as_deref(), Some("Redis"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_vite_ports() {
        let (svc1, _) = identify_service(5173, "unknown");
        let (svc2, _) = identify_service(5174, "unknown");
        assert_eq!(svc1.as_deref(), Some("Vite"));
        assert_eq!(svc2.as_deref(), Some("Vite"));
    }

    #[test]
    fn identifies_port_22() {
        let (svc, cat) = identify_service(22, "unknown");
        assert_eq!(svc.as_deref(), Some("SSH"));
        assert_eq!(cat, ServiceCategory::System);
    }

    // --- Priority: process name beats port ---

    #[test]
    fn process_name_takes_priority_over_port() {
        // Port 3000 would map to "Next.js / Rails", but "postgres" name should win
        let (svc, cat) = identify_service(3000, "postgres");
        assert_eq!(svc.as_deref(), Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    // --- Case insensitivity ---

    #[test]
    fn process_name_is_case_insensitive() {
        let (svc, _) = identify_service(9999, "POSTGRES");
        assert_eq!(svc.as_deref(), Some("PostgreSQL"));

        let (svc, _) = identify_service(9999, "Redis-Server");
        assert_eq!(svc.as_deref(), Some("Redis"));

        let (svc, _) = identify_service(9999, "NGINX");
        assert_eq!(svc.as_deref(), Some("Nginx"));
    }

    // --- New services ---

    #[test]
    fn identifies_bun() {
        let (svc, cat) = identify_service(9999, "bun");
        assert_eq!(svc.as_deref(), Some("Bun"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_deno() {
        let (svc, cat) = identify_service(9999, "deno");
        assert_eq!(svc.as_deref(), Some("Deno"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_supabase() {
        let (svc, cat) = identify_service(9999, "supabase");
        assert_eq!(svc.as_deref(), Some("Supabase"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_minio() {
        let (svc, cat) = identify_service(9999, "minio");
        assert_eq!(svc.as_deref(), Some("MinIO"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_clickhouse() {
        let (svc, cat) = identify_service(9999, "clickhouse-server");
        assert_eq!(svc.as_deref(), Some("ClickHouse"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_elasticsearch() {
        let (svc, cat) = identify_service(9999, "elasticsearch");
        assert_eq!(svc.as_deref(), Some("Elasticsearch"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_opensearch() {
        let (svc, cat) = identify_service(9999, "opensearch");
        assert_eq!(svc.as_deref(), Some("OpenSearch"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_traefik() {
        let (svc, cat) = identify_service(9999, "traefik");
        assert_eq!(svc.as_deref(), Some("Traefik"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_envoy() {
        let (svc, cat) = identify_service(9999, "envoy");
        assert_eq!(svc.as_deref(), Some("Envoy"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_haproxy() {
        let (svc, cat) = identify_service(9999, "haproxy");
        assert_eq!(svc.as_deref(), Some("HAProxy"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_kafka_by_name() {
        let (svc, cat) = identify_service(9999, "kafka");
        assert_eq!(svc.as_deref(), Some("Kafka"));
        assert_eq!(cat, ServiceCategory::Queue);
    }

    #[test]
    fn identifies_rabbitmq_by_name() {
        let (svc, cat) = identify_service(9999, "rabbitmq-server");
        assert_eq!(svc.as_deref(), Some("RabbitMQ"));
        assert_eq!(cat, ServiceCategory::Queue);
    }

    #[test]
    fn identifies_nats() {
        let (svc, cat) = identify_service(9999, "nats-server");
        assert_eq!(svc.as_deref(), Some("NATS"));
        assert_eq!(cat, ServiceCategory::Queue);
    }

    #[test]
    fn identifies_kubernetes() {
        let (svc, cat) = identify_service(9999, "kubelet");
        assert_eq!(svc.as_deref(), Some("Kubernetes"));
        assert_eq!(cat, ServiceCategory::Container);
    }

    #[test]
    fn identifies_port_9200_elasticsearch() {
        let (svc, cat) = identify_service(9200, "unknown");
        assert_eq!(svc.as_deref(), Some("Elasticsearch"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_port_8123_clickhouse() {
        let (svc, cat) = identify_service(8123, "unknown");
        assert_eq!(svc.as_deref(), Some("ClickHouse"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    // --- Unknown ---

    #[test]
    fn unknown_process_and_port() {
        let (svc, cat) = identify_service(12345, "my_custom_app");
        assert_eq!(svc, None);
        assert_eq!(cat, ServiceCategory::Unknown);
    }
}

#[cfg(test)]
mod v2_tests {
    use super::*;
    use crate::models::testutil;
    use crate::models::ServiceCategory;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn ident(port: u16, name: &str, cmd: &[&str]) -> Service {
        identify(port, name, &args(cmd))
            .unwrap_or_else(|| panic!("no service for {name} {cmd:?} on {port}"))
    }

    #[test]
    fn command_rules_win_over_port() {
        let s = ident(
            3000,
            "node",
            &["node", "/app/node_modules/next/dist/bin/next", "dev"],
        );
        assert_eq!(s.name, "Next.js");
        assert_eq!(s.source, ServiceSource::Command);
        assert_eq!(s.category, ServiceCategory::DevServer);
    }

    #[test]
    fn realpathed_npm_bins_are_detected() {
        let s = ident(
            5173,
            "node",
            &["node", "/app/node_modules/vite/bin/vite.js"],
        );
        assert_eq!(s.name, "Vite");
        let s = ident(
            6006,
            "node",
            &["node", "/app/node_modules/storybook/bin/index.cjs", "dev"],
        );
        assert_eq!(s.name, "Storybook");
        let s = ident(
            4200,
            "node",
            &["node", "/app/node_modules/@angular/cli/bin/ng.js", "serve"],
        );
        assert_eq!(s.name, "Angular");
    }

    #[test]
    fn python_frameworks_from_command() {
        assert_eq!(
            ident(8000, "Python", &["python3", "manage.py", "runserver"]).name,
            "Django"
        );
        assert_eq!(
            ident(
                8000,
                "python3.12",
                &["/venv/bin/python", "/venv/bin/uvicorn", "main:app"]
            )
            .name,
            "Uvicorn"
        );
        assert_eq!(
            ident(5000, "python3", &["python3", "-m", "flask", "run"]).name,
            "Flask"
        );
    }

    #[test]
    fn process_titles_are_detected() {
        assert_eq!(ident(3000, "next-server (v14.2.3)", &[]).name, "Next.js");
        assert_eq!(ident(8000, "gunicorn: master [app]", &[]).name, "Gunicorn");
        assert_eq!(
            ident(3000, "ruby", &["puma 6.4.2 (tcp://localhost:3000) [shop]"]).name,
            "Puma"
        );
    }

    #[test]
    fn command_rules_ignore_non_runtime_processes() {
        let s = ident(
            51234,
            "Code Helper (Plugin)",
            &["/app/Code Helper", "/x/node_modules/vite/dist/node/cli.js"],
        );
        assert_eq!(s.name, "VS Code");
    }

    #[test]
    fn java_kafka_detected_from_command() {
        let s = ident(
            9092,
            "java",
            &[
                "java",
                "-Xmx1G",
                "-cp",
                "/opt/kafka/libs/*",
                "kafka.Kafka",
                "server.properties",
            ],
        );
        assert_eq!(s.name, "Kafka");
        assert_eq!(s.category, ServiceCategory::Queue);
        assert_eq!(s.source, ServiceSource::Command);
    }

    #[test]
    fn elasticsearch_from_java_command() {
        let s = ident(
            9200,
            "java",
            &[
                "/usr/share/es/jdk/bin/java",
                "org.elasticsearch.bootstrap.Elasticsearch",
            ],
        );
        assert_eq!(s.name, "Elasticsearch");
        assert_eq!(s.category, ServiceCategory::Database);
    }

    #[test]
    fn linux_chrome_names_are_browsers() {
        for (name, svc) in [
            ("chrome", "Chrome"),
            ("chromium", "Chromium"),
            ("brave", "Brave"),
        ] {
            let s = ident(40000, name, &[]);
            assert_eq!(s.name, svc, "{name}");
            assert_eq!(s.category, ServiceCategory::Browser, "{name}");
        }
    }

    #[test]
    fn port_9100_is_node_exporter() {
        let s = ident(9100, "unknown", &[]);
        assert_eq!(s.name, "node_exporter");
        assert_eq!(s.source, ServiceSource::Port);
    }

    #[test]
    fn port_only_guess_has_port_source() {
        let s = ident(8000, "Python", &["python3", "app.py"]);
        assert_eq!(s.source, ServiceSource::Port);
        let mut e = testutil::entry(8000, 1, "Python");
        e.service = Some(s);
        assert!(e.service_display().ends_with('?'));
    }

    #[test]
    fn airplay_has_note() {
        let s = ident(5000, "ControlCenter", &[]);
        assert_eq!(s.name, "AirPlay");
        assert!(s.note.unwrap_or_default().contains("AirPlay Receiver"));
    }

    #[test]
    fn docker_backend_is_container_proxy() {
        assert!(is_container_proxy("com.docker.backend", &[]));
        assert!(is_container_proxy(
            "docker-proxy",
            &args(&["-host-port", "5432"])
        ));
        assert!(is_container_proxy("limactl", &[]));
        assert!(!is_container_proxy("node", &[]));
        assert!(ident(6333, "com.docker.backend", &[]).note.is_some());
    }

    #[test]
    fn queue_category_for_brokers() {
        for name in ["kafka", "rabbitmq-server", "beam.smp", "nats-server"] {
            assert_eq!(
                ident(1, name, &[]).category,
                ServiceCategory::Queue,
                "{name}"
            );
        }
        assert_eq!(ident(5672, "unknown", &[]).category, ServiceCategory::Queue);
    }

    #[test]
    fn dev_ports_are_known() {
        let table = [
            (5000, "Flask"),
            (8081, "Metro"),
            (6006, "Storybook"),
            (4321, "Astro"),
            (1313, "Hugo"),
            (9229, "Node inspector"),
            (4566, "LocalStack"),
            (54321, "Supabase API"),
            (54322, "Supabase DB"),
            (54323, "Supabase Studio"),
            (6333, "Qdrant"),
            (7700, "Meilisearch"),
            (1234, "LM Studio"),
            (8501, "Streamlit"),
            (7860, "Gradio"),
            (9001, "MinIO Console"),
            (5601, "Kibana"),
            (16686, "Jaeger"),
            (4317, "OTLP gRPC"),
            (4318, "OTLP HTTP"),
            (1025, "Mailpit SMTP"),
            (8025, "Mailpit"),
            (8787, "Wrangler"),
        ];
        for (port, name) in table {
            let s = ident(port, "unknown", &[]);
            assert_eq!(s.name, name, "port {port}");
            assert_eq!(s.source, ServiceSource::Port, "port {port}");
        }
    }

    #[test]
    fn http_and_https_ports() {
        assert!(is_https_port(443));
        assert!(is_https_port(8443));
        assert!(!is_https_port(8080));
        let pg = ident(5432, "postgres", &[]);
        assert!(!is_http_like(5432, Some(&pg)));
        let next = ident(
            3000,
            "node",
            &["node", "/a/node_modules/next/dist/bin/next"],
        );
        assert!(is_http_like(3000, Some(&next)));
        assert!(is_http_like(15672, None), "RabbitMQ UI is a web port");
        assert!(!is_http_like(49999, None));
    }
}
