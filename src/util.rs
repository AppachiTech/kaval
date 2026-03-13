use crate::models::ServiceCategory;

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

/// Process name → (service, category) mapping table.
/// Order matters: first match wins.
const PROCESS_RULES: &[(Match, &str, ServiceCategory)] = &[
    // Databases
    (
        Match::Contains("postgres"),
        "PostgreSQL",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("postmaster"),
        "PostgreSQL",
        ServiceCategory::Database,
    ),
    (Match::Contains("mysql"), "MySQL", ServiceCategory::Database),
    (
        Match::Contains("mariadbd"),
        "MySQL",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("mariadb"),
        "MySQL",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("mongod"),
        "MongoDB",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("mongos"),
        "MongoDB",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("clickhouse"),
        "ClickHouse",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("elasticsearch"),
        "Elasticsearch",
        ServiceCategory::Database,
    ),
    (
        Match::Contains("opensearch"),
        "Elasticsearch",
        ServiceCategory::Database,
    ),
    (Match::Contains("minio"), "MinIO", ServiceCategory::Database),
    // Cache / message brokers
    (
        Match::Contains("redis-server"),
        "Redis",
        ServiceCategory::Cache,
    ),
    (Match::Exact("redis"), "Redis", ServiceCategory::Cache),
    (
        Match::Contains("memcached"),
        "Memcached",
        ServiceCategory::Cache,
    ),
    (Match::Contains("kafka"), "Kafka", ServiceCategory::Cache),
    (
        Match::Contains("rabbitmq"),
        "RabbitMQ",
        ServiceCategory::Cache,
    ),
    (
        Match::Contains("beam.smp"),
        "RabbitMQ",
        ServiceCategory::Cache,
    ),
    (
        Match::Contains("nats-server"),
        "NATS",
        ServiceCategory::Cache,
    ),
    (Match::Exact("nats"), "NATS", ServiceCategory::Cache),
    // Containers
    (
        Match::Contains("docker"),
        "Docker",
        ServiceCategory::Container,
    ),
    (
        Match::Contains("containerd"),
        "Docker",
        ServiceCategory::Container,
    ),
    (
        Match::Contains("colima"),
        "Colima",
        ServiceCategory::Container,
    ),
    (
        Match::Contains("kubectl"),
        "Kubernetes",
        ServiceCategory::Container,
    ),
    (
        Match::Contains("kubelet"),
        "Kubernetes",
        ServiceCategory::Container,
    ),
    // Dev tools / runtimes
    (Match::Exact("bun"), "Bun", ServiceCategory::DevServer),
    (Match::Prefix("bun "), "Bun", ServiceCategory::DevServer),
    (Match::Exact("deno"), "Deno", ServiceCategory::DevServer),
    (Match::Prefix("deno "), "Deno", ServiceCategory::DevServer),
    (
        Match::Contains("ollama"),
        "Ollama",
        ServiceCategory::DevServer,
    ),
    (
        Match::Contains("supabase"),
        "Supabase",
        ServiceCategory::DevServer,
    ),
    (
        Match::Contains("code helper"),
        "VS Code",
        ServiceCategory::DevServer,
    ),
    (
        Match::Prefix("code - "),
        "VS Code",
        ServiceCategory::DevServer,
    ),
    (
        Match::Both("cursor", "helper"),
        "Cursor",
        ServiceCategory::DevServer,
    ),
    (
        Match::Contains("electron"),
        "Electron",
        ServiceCategory::DevServer,
    ),
    // Browsers
    (
        Match::Contains("brave browser"),
        "Brave",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("brave helper"),
        "Brave",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("google chrome"),
        "Chrome",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("chrome helper"),
        "Chrome",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("firefox"),
        "Firefox",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("geckodriver"),
        "Firefox",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("safari"),
        "Safari",
        ServiceCategory::Browser,
    ),
    (
        Match::Contains("webkit"),
        "Safari",
        ServiceCategory::Browser,
    ),
    // System / infrastructure
    (Match::Contains("sshd"), "SSH", ServiceCategory::System),
    (Match::Contains("nginx"), "Nginx", ServiceCategory::System),
    (Match::Contains("httpd"), "Apache", ServiceCategory::System),
    (Match::Contains("apache"), "Apache", ServiceCategory::System),
    (
        Match::Contains("controlcenter"),
        "AirPlay",
        ServiceCategory::System,
    ),
    (
        Match::Contains("sharingd"),
        "Sharing",
        ServiceCategory::System,
    ),
    (
        Match::Contains("rapportd"),
        "Rapport",
        ServiceCategory::System,
    ),
    (
        Match::Contains("identityservicesd"),
        "Identity",
        ServiceCategory::System,
    ),
    (
        Match::Contains("grafana"),
        "Grafana",
        ServiceCategory::System,
    ),
    (
        Match::Contains("prometheus"),
        "Prometheus",
        ServiceCategory::System,
    ),
    (Match::Contains("caddy"), "Caddy", ServiceCategory::System),
    (
        Match::Contains("traefik"),
        "Traefik",
        ServiceCategory::System,
    ),
    (Match::Contains("envoy"), "Envoy", ServiceCategory::System),
    (
        Match::Contains("haproxy"),
        "HAProxy",
        ServiceCategory::System,
    ),
];

/// Port → (service, category) mapping table.
const PORT_RULES: &[(u16, &str, ServiceCategory)] = &[
    // Dev servers
    (3000, "Next.js / Rails", ServiceCategory::DevServer),
    (3001, "React Dev", ServiceCategory::DevServer),
    (4000, "Phoenix", ServiceCategory::DevServer),
    (4200, "Angular", ServiceCategory::DevServer),
    (5173, "Vite", ServiceCategory::DevServer),
    (5174, "Vite", ServiceCategory::DevServer),
    (8000, "Django / FastAPI", ServiceCategory::DevServer),
    (8080, "HTTP Alt", ServiceCategory::DevServer),
    (8443, "HTTPS Alt", ServiceCategory::DevServer),
    (8888, "Jupyter", ServiceCategory::DevServer),
    (9000, "PHP-FPM", ServiceCategory::DevServer),
    (19006, "Expo", ServiceCategory::DevServer),
    // Databases
    (3306, "MySQL", ServiceCategory::Database),
    (5432, "PostgreSQL", ServiceCategory::Database),
    (5433, "PostgreSQL Alt", ServiceCategory::Database),
    (27017, "MongoDB", ServiceCategory::Database),
    (26257, "CockroachDB", ServiceCategory::Database),
    (9200, "Elasticsearch", ServiceCategory::Database),
    (8123, "ClickHouse", ServiceCategory::Database),
    (9004, "ClickHouse", ServiceCategory::Database),
    (9100, "MinIO", ServiceCategory::Database),
    // Cache / brokers
    (6379, "Redis", ServiceCategory::Cache),
    (11211, "Memcached", ServiceCategory::Cache),
    (9092, "Kafka", ServiceCategory::Cache),
    (5672, "RabbitMQ", ServiceCategory::Cache),
    (15672, "RabbitMQ UI", ServiceCategory::Cache),
    (4222, "NATS", ServiceCategory::Cache),
    // Container / orchestration
    (2375, "Docker", ServiceCategory::Container),
    (2376, "Docker", ServiceCategory::Container),
    // Dev tools
    (11434, "Ollama", ServiceCategory::DevServer),
    // System
    (22, "SSH", ServiceCategory::System),
    (80, "HTTP", ServiceCategory::System),
    (443, "HTTPS", ServiceCategory::System),
    (53, "DNS", ServiceCategory::System),
    (9090, "Prometheus", ServiceCategory::System),
    (2019, "Caddy Admin", ServiceCategory::System),
];

fn matches_rule(name: &str, m: Match) -> bool {
    match m {
        Match::Contains(p) => name.contains(p),
        Match::Exact(p) => name == p,
        Match::Prefix(p) => name.starts_with(p),
        Match::Both(a, b) => name.contains(a) && name.contains(b),
    }
}

/// Known port-to-service mappings for common developer tools
pub fn identify_service(port: u16, process_name: &str) -> (Option<&'static str>, ServiceCategory) {
    let name_lower = process_name.to_lowercase();

    // First try process name detection (more reliable than port)
    for &(ref matcher, service, category) in PROCESS_RULES {
        if matches_rule(&name_lower, *matcher) {
            return (Some(service), category);
        }
    }

    // Fall back to well-known port mappings
    for &(p, service, category) in PORT_RULES {
        if port == p {
            return (Some(service), category);
        }
    }

    (None, ServiceCategory::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Process name detection (databases) ---

    #[test]
    fn identifies_postgres_by_name() {
        let (svc, cat) = identify_service(9999, "postgres");
        assert_eq!(svc, Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_postmaster() {
        let (svc, cat) = identify_service(9999, "postmaster");
        assert_eq!(svc, Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_mysql_by_name() {
        let (svc, cat) = identify_service(9999, "mysqld");
        assert_eq!(svc, Some("MySQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_mariadb_by_name() {
        let (svc, cat) = identify_service(9999, "mariadbd");
        assert_eq!(svc, Some("MySQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_mongodb_by_name() {
        let (svc, cat) = identify_service(9999, "mongod");
        assert_eq!(svc, Some("MongoDB"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    // --- Process name detection (cache) ---

    #[test]
    fn identifies_redis_server() {
        let (svc, cat) = identify_service(9999, "redis-server");
        assert_eq!(svc, Some("Redis"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_memcached() {
        let (svc, cat) = identify_service(9999, "memcached");
        assert_eq!(svc, Some("Memcached"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    // --- Process name detection (containers) ---

    #[test]
    fn identifies_docker() {
        let (svc, cat) = identify_service(9999, "com.docker.backend");
        assert_eq!(svc, Some("Docker"));
        assert_eq!(cat, ServiceCategory::Container);
    }

    #[test]
    fn identifies_colima() {
        let (svc, cat) = identify_service(9999, "colima");
        assert_eq!(svc, Some("Colima"));
        assert_eq!(cat, ServiceCategory::Container);
    }

    // --- Process name detection (dev tools) ---

    #[test]
    fn identifies_vscode() {
        let (svc, cat) = identify_service(9999, "Code Helper");
        assert_eq!(svc, Some("VS Code"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_cursor() {
        let (svc, cat) = identify_service(9999, "Cursor Helper");
        assert_eq!(svc, Some("Cursor"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_ollama() {
        let (svc, cat) = identify_service(9999, "ollama");
        assert_eq!(svc, Some("Ollama"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    // --- Process name detection (browsers) ---

    #[test]
    fn identifies_chrome() {
        let (svc, cat) = identify_service(9999, "Google Chrome Helper");
        assert_eq!(svc, Some("Chrome"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    #[test]
    fn identifies_brave() {
        let (svc, cat) = identify_service(9999, "Brave Browser Helper");
        assert_eq!(svc, Some("Brave"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    #[test]
    fn identifies_firefox() {
        let (svc, cat) = identify_service(9999, "firefox");
        assert_eq!(svc, Some("Firefox"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    #[test]
    fn identifies_safari() {
        let (svc, cat) = identify_service(9999, "Safari");
        assert_eq!(svc, Some("Safari"));
        assert_eq!(cat, ServiceCategory::Browser);
    }

    // --- Process name detection (system) ---

    #[test]
    fn identifies_nginx() {
        let (svc, cat) = identify_service(9999, "nginx");
        assert_eq!(svc, Some("Nginx"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_sshd() {
        let (svc, cat) = identify_service(9999, "sshd");
        assert_eq!(svc, Some("SSH"));
        assert_eq!(cat, ServiceCategory::System);
    }

    // --- Port-based detection ---

    #[test]
    fn identifies_port_3000() {
        let (svc, cat) = identify_service(3000, "node");
        assert_eq!(svc, Some("Next.js / Rails"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_port_5432() {
        let (svc, cat) = identify_service(5432, "unknown_process");
        assert_eq!(svc, Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_port_6379() {
        let (svc, cat) = identify_service(6379, "unknown_process");
        assert_eq!(svc, Some("Redis"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_vite_ports() {
        let (svc1, _) = identify_service(5173, "unknown");
        let (svc2, _) = identify_service(5174, "unknown");
        assert_eq!(svc1, Some("Vite"));
        assert_eq!(svc2, Some("Vite"));
    }

    #[test]
    fn identifies_port_22() {
        let (svc, cat) = identify_service(22, "unknown");
        assert_eq!(svc, Some("SSH"));
        assert_eq!(cat, ServiceCategory::System);
    }

    // --- Priority: process name beats port ---

    #[test]
    fn process_name_takes_priority_over_port() {
        // Port 3000 would map to "Next.js / Rails", but "postgres" name should win
        let (svc, cat) = identify_service(3000, "postgres");
        assert_eq!(svc, Some("PostgreSQL"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    // --- Case insensitivity ---

    #[test]
    fn process_name_is_case_insensitive() {
        let (svc, _) = identify_service(9999, "POSTGRES");
        assert_eq!(svc, Some("PostgreSQL"));

        let (svc, _) = identify_service(9999, "Redis-Server");
        assert_eq!(svc, Some("Redis"));

        let (svc, _) = identify_service(9999, "NGINX");
        assert_eq!(svc, Some("Nginx"));
    }

    // --- New services ---

    #[test]
    fn identifies_bun() {
        let (svc, cat) = identify_service(9999, "bun");
        assert_eq!(svc, Some("Bun"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_deno() {
        let (svc, cat) = identify_service(9999, "deno");
        assert_eq!(svc, Some("Deno"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_supabase() {
        let (svc, cat) = identify_service(9999, "supabase");
        assert_eq!(svc, Some("Supabase"));
        assert_eq!(cat, ServiceCategory::DevServer);
    }

    #[test]
    fn identifies_minio() {
        let (svc, cat) = identify_service(9999, "minio");
        assert_eq!(svc, Some("MinIO"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_clickhouse() {
        let (svc, cat) = identify_service(9999, "clickhouse-server");
        assert_eq!(svc, Some("ClickHouse"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_elasticsearch() {
        let (svc, cat) = identify_service(9999, "elasticsearch");
        assert_eq!(svc, Some("Elasticsearch"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_opensearch() {
        let (svc, cat) = identify_service(9999, "opensearch");
        assert_eq!(svc, Some("Elasticsearch"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_traefik() {
        let (svc, cat) = identify_service(9999, "traefik");
        assert_eq!(svc, Some("Traefik"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_envoy() {
        let (svc, cat) = identify_service(9999, "envoy");
        assert_eq!(svc, Some("Envoy"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_haproxy() {
        let (svc, cat) = identify_service(9999, "haproxy");
        assert_eq!(svc, Some("HAProxy"));
        assert_eq!(cat, ServiceCategory::System);
    }

    #[test]
    fn identifies_kafka_by_name() {
        let (svc, cat) = identify_service(9999, "kafka");
        assert_eq!(svc, Some("Kafka"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_rabbitmq_by_name() {
        let (svc, cat) = identify_service(9999, "rabbitmq-server");
        assert_eq!(svc, Some("RabbitMQ"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_nats() {
        let (svc, cat) = identify_service(9999, "nats-server");
        assert_eq!(svc, Some("NATS"));
        assert_eq!(cat, ServiceCategory::Cache);
    }

    #[test]
    fn identifies_kubernetes() {
        let (svc, cat) = identify_service(9999, "kubelet");
        assert_eq!(svc, Some("Kubernetes"));
        assert_eq!(cat, ServiceCategory::Container);
    }

    #[test]
    fn identifies_port_9200_elasticsearch() {
        let (svc, cat) = identify_service(9200, "unknown");
        assert_eq!(svc, Some("Elasticsearch"));
        assert_eq!(cat, ServiceCategory::Database);
    }

    #[test]
    fn identifies_port_8123_clickhouse() {
        let (svc, cat) = identify_service(8123, "unknown");
        assert_eq!(svc, Some("ClickHouse"));
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
