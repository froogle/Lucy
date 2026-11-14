//! Plugin credentials: the values a Plugin's Manifest declares by name,
//! kept in the Secret Service. Lucy stores no secrets of her own. `lucy`
//! writes them as the user enters them; lucyd reads them when it starts an
//! approved Plugin. They never pass through config, IPC or the model.

/// Where Plugin credentials are kept. Blocking; lucyd calls it off the
/// async runtime.
pub trait CredentialStore: Send + Sync {
    /// The value of `plugin`'s credential `name`, if the user has entered
    /// one. `Err` says why the store couldn't be read.
    fn get(&self, plugin: &str, name: &str) -> Result<Option<String>, String>;
    /// Keeps `value` as `plugin`'s credential `name`, replacing any before.
    fn set(&self, plugin: &str, name: &str, value: &str) -> Result<(), String>;
}

/// The user's Secret Service (through `oo7`), its default collection.
pub struct SecretService;

impl SecretService {
    fn run<T>(&self, work: impl std::future::Future<Output = oo7::Result<T>>) -> Result<T, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime
            .block_on(work)
            .map_err(|e| format!("the Secret Service: {e}"))
    }
}

/// The attributes an item is kept under.
fn attributes<'a>(plugin: &'a str, name: &'a str) -> [(&'static str, &'a str); 3] {
    [
        ("application", "lucy"),
        ("lucy-plugin", plugin),
        ("lucy-credential", name),
    ]
}

impl CredentialStore for SecretService {
    fn get(&self, plugin: &str, name: &str) -> Result<Option<String>, String> {
        self.run(async {
            let keyring = oo7::Keyring::new().await?;
            let Some(item) = keyring
                .search_items(&attributes(plugin, name))
                .await?
                .into_iter()
                .next()
            else {
                return Ok(None);
            };
            if item.is_locked().await? {
                item.unlock().await?;
            }
            let secret = item.secret().await?;
            Ok(Some(
                String::from_utf8_lossy(secret.as_bytes()).into_owned(),
            ))
        })
    }

    fn set(&self, plugin: &str, name: &str, value: &str) -> Result<(), String> {
        self.run(async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            let label = format!("Lucy: {name} for the {plugin} Plugin");
            keyring
                .create_item(&label, &attributes(plugin, name), value, true)
                .await
        })
    }
}
