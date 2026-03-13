use crate::models::ServiceCategory;

/// Known port-to-service mappings for common developer tools
pub fn identify_service(port: u16, process_name: &str) -> (Option<&'static str>, ServiceCategory) {
    // First try process name detection (more reliable than port)
    let name_lower = process_name.to_lowercase();

    if let Some(result) = identify_by_process_name(&name_lower) {
        return result;
    }

    // Fall back to well-known port mappings
    identify_by_port(port)
}

fn identify_by_process_name(name: &str) -> Option<(Option<&'static str>, ServiceCategory)> {
    // Databases
    if name.contains("postgres") || name.contains("postmaster") {
        return Some((Some("PostgreSQL"), ServiceCategory::Database));
    }
    if name.contains("mysql") || name.contains("mariadbd") || name.contains("mariadb") {
        return Some((Some("MySQL"), ServiceCategory::Database));
    }
    if name.contains("mongod") || name.contains("mongos") {
        return Some((Some("MongoDB"), ServiceCategory::Database));
    }

    // Cache / message brokers
    if name.contains("redis-server") || name == "redis" {
        return Some((Some("Redis"), ServiceCategory::Cache));
    }
    if name.contains("memcached") {
        return Some((Some("Memcached"), ServiceCategory::Cache));
    }

    // Containers
    if name.contains("docker") || name.contains("containerd") {
        return Some((Some("Docker"), ServiceCategory::Container));
    }
    if name.contains("colima") {
        return Some((Some("Colima"), ServiceCategory::Container));
    }

    // Dev tools
    if name.contains("ollama") {
        return Some((Some("Ollama"), ServiceCategory::DevServer));
    }
    if name.contains("code helper") || name.starts_with("code - ") {
        return Some((Some("VS Code"), ServiceCategory::DevServer));
    }
    if name.contains("cursor") && name.contains("helper") {
        return Some((Some("Cursor"), ServiceCategory::DevServer));
    }
    if name.contains("electron") {
        return Some((Some("Electron"), ServiceCategory::DevServer));
    }

    // Browsers
    if name.contains("brave browser") || name.contains("brave helper") {
        return Some((Some("Brave"), ServiceCategory::Browser));
    }
    if name.contains("google chrome") || name.contains("chrome helper") {
        return Some((Some("Chrome"), ServiceCategory::Browser));
    }
    if name.contains("firefox") || name.contains("geckodriver") {
        return Some((Some("Firefox"), ServiceCategory::Browser));
    }
    if name.contains("safari") || name.contains("webkit") {
        return Some((Some("Safari"), ServiceCategory::Browser));
    }

    // System services
    if name.contains("sshd") {
        return Some((Some("SSH"), ServiceCategory::System));
    }
    if name == "nginx" || name.contains("nginx") {
        return Some((Some("Nginx"), ServiceCategory::System));
    }
    if name.contains("httpd") || name.contains("apache") {
        return Some((Some("Apache"), ServiceCategory::System));
    }
    if name.contains("controlcenter") {
        return Some((Some("AirPlay"), ServiceCategory::System));
    }
    if name.contains("sharingd") {
        return Some((Some("Sharing"), ServiceCategory::System));
    }
    if name.contains("rapportd") {
        return Some((Some("Rapport"), ServiceCategory::System));
    }
    if name.contains("identityservicesd") {
        return Some((Some("Identity"), ServiceCategory::System));
    }
    if name.contains("grafana") {
        return Some((Some("Grafana"), ServiceCategory::System));
    }
    if name.contains("prometheus") {
        return Some((Some("Prometheus"), ServiceCategory::System));
    }
    if name.contains("caddy") {
        return Some((Some("Caddy"), ServiceCategory::System));
    }

    None
}

fn identify_by_port(port: u16) -> (Option<&'static str>, ServiceCategory) {
    match port {
        // Dev servers
        3000 => (Some("Next.js / Rails"), ServiceCategory::DevServer),
        3001 => (Some("React Dev"), ServiceCategory::DevServer),
        4000 => (Some("Phoenix"), ServiceCategory::DevServer),
        4200 => (Some("Angular"), ServiceCategory::DevServer),
        5173 | 5174 => (Some("Vite"), ServiceCategory::DevServer),
        8000 => (Some("Django / FastAPI"), ServiceCategory::DevServer),
        8080 => (Some("HTTP Alt"), ServiceCategory::DevServer),
        8443 => (Some("HTTPS Alt"), ServiceCategory::DevServer),
        8888 => (Some("Jupyter"), ServiceCategory::DevServer),
        9000 => (Some("PHP-FPM"), ServiceCategory::DevServer),
        19006 => (Some("Expo"), ServiceCategory::DevServer),

        // Databases
        3306 => (Some("MySQL"), ServiceCategory::Database),
        5432 => (Some("PostgreSQL"), ServiceCategory::Database),
        5433 => (Some("PostgreSQL Alt"), ServiceCategory::Database),
        27017 => (Some("MongoDB"), ServiceCategory::Database),
        26257 => (Some("CockroachDB"), ServiceCategory::Database),

        // Cache / brokers
        6379 => (Some("Redis"), ServiceCategory::Cache),
        11211 => (Some("Memcached"), ServiceCategory::Cache),
        9092 => (Some("Kafka"), ServiceCategory::Cache),
        5672 => (Some("RabbitMQ"), ServiceCategory::Cache),
        15672 => (Some("RabbitMQ UI"), ServiceCategory::Cache),

        // Container / orchestration
        2375 | 2376 => (Some("Docker"), ServiceCategory::Container),

        // Dev tools
        11434 => (Some("Ollama"), ServiceCategory::DevServer),

        // System
        22 => (Some("SSH"), ServiceCategory::System),
        80 => (Some("HTTP"), ServiceCategory::System),
        443 => (Some("HTTPS"), ServiceCategory::System),
        53 => (Some("DNS"), ServiceCategory::System),
        9090 => (Some("Prometheus"), ServiceCategory::System),
        2019 => (Some("Caddy Admin"), ServiceCategory::System),

        _ => (None, ServiceCategory::Unknown),
    }
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

    // --- Unknown ---

    #[test]
    fn unknown_process_and_port() {
        let (svc, cat) = identify_service(12345, "my_custom_app");
        assert_eq!(svc, None);
        assert_eq!(cat, ServiceCategory::Unknown);
    }
}
