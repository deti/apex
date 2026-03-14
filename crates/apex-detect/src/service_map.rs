//! Service Dependency Mapping — discovers runtime service dependencies from code.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub enum DependencyKind {
    Http,
    Grpc,
    MessageQueue,
    Database,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDependency {
    pub kind: DependencyKind,
    pub target: String,
    pub file: PathBuf,
    pub line: u32,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceMap {
    pub dependencies: Vec<ServiceDependency>,
    pub http_count: usize,
    pub grpc_count: usize,
    pub mq_count: usize,
    pub db_count: usize,
}

static HTTP_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r#"requests\.(get|post|put|delete|patch)\s*\("#).unwrap(),
        Regex::new(r#"httpx\.(get|post|put|delete|patch)\s*\("#).unwrap(),
        Regex::new(r#"fetch\s*\(\s*['"`]"#).unwrap(),
        Regex::new(r#"axios\.(get|post|put|delete|patch)\s*\("#).unwrap(),
        Regex::new(r"reqwest::Client|hyper::Client").unwrap(),
    ]
});

static MQ_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"KafkaProducer|KafkaConsumer|NatsClient").unwrap(),
        Regex::new(r#"channel\.(basic_publish|basic_consume)\s*\("#).unwrap(),
    ]
});

static DB_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"create_engine|MongoClient|redis\.Redis").unwrap(),
        Regex::new(r"DATABASE_URL|MONGO_URI|REDIS_URL").unwrap(),
    ]
});

static GRPC_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r#"grpc\.(insecure_channel|secure_channel)\s*\("#).unwrap(),
        Regex::new(r"Stub\s*\(").unwrap(),
    ]
});

pub fn analyze_service_map(source_cache: &HashMap<PathBuf, String>) -> ServiceMap {
    let mut deps = Vec::new();

    for (path, source) in source_cache {
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let ln = (line_num + 1) as u32;

            for re in HTTP_RE.iter() {
                if re.is_match(trimmed) {
                    deps.push(ServiceDependency {
                        kind: DependencyKind::Http,
                        target: String::new(),
                        file: path.clone(),
                        line: ln,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
            for re in MQ_RE.iter() {
                if re.is_match(trimmed) {
                    deps.push(ServiceDependency {
                        kind: DependencyKind::MessageQueue,
                        target: String::new(),
                        file: path.clone(),
                        line: ln,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
            for re in DB_RE.iter() {
                if re.is_match(trimmed) {
                    deps.push(ServiceDependency {
                        kind: DependencyKind::Database,
                        target: String::new(),
                        file: path.clone(),
                        line: ln,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
            for re in GRPC_RE.iter() {
                if re.is_match(trimmed) {
                    deps.push(ServiceDependency {
                        kind: DependencyKind::Grpc,
                        target: String::new(),
                        file: path.clone(),
                        line: ln,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
        }
    }

    let http_count = deps
        .iter()
        .filter(|d| matches!(d.kind, DependencyKind::Http))
        .count();
    let grpc_count = deps
        .iter()
        .filter(|d| matches!(d.kind, DependencyKind::Grpc))
        .count();
    let mq_count = deps
        .iter()
        .filter(|d| matches!(d.kind, DependencyKind::MessageQueue))
        .count();
    let db_count = deps
        .iter()
        .filter(|d| matches!(d.kind, DependencyKind::Database))
        .count();

    ServiceMap {
        dependencies: deps,
        http_count,
        grpc_count,
        mq_count,
        db_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_python_requests() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("client.py"),
            "resp = requests.get(url)".into(),
        );
        let m = analyze_service_map(&c);
        assert_eq!(m.http_count, 1);
    }

    #[test]
    fn detects_database() {
        let mut c = HashMap::new();
        c.insert(
            PathBuf::from("db.py"),
            "engine = create_engine(url)".into(),
        );
        let m = analyze_service_map(&c);
        assert_eq!(m.db_count, 1);
    }

    #[test]
    fn empty_source_empty_map() {
        let m = analyze_service_map(&HashMap::new());
        assert_eq!(m.dependencies.len(), 0);
    }
}
