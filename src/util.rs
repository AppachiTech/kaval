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
