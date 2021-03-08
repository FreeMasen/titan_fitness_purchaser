use std::str::FromStr;

use fantoccini::{elements::Element, ClientBuilder, Locator};
use structopt::StructOpt;
use tokio::io::AsyncReadExt;

type R<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, StructOpt)]
#[structopt(name = "tfp", about = "A titan.fitness auto purchaser")]
struct Opts {
    #[structopt(short, long)]
    /// The full url for the item to purchase
    pub url: String,
    #[structopt(short, long)]
    /// The username and password to log in with <username>:<password>
    pub account_info: AccountInfo,
    #[structopt(short, long)]
    /// The price to not exceed
    pub price: f32,
    #[structopt(short, long)]
    /// If an option exists for the item, the index of the value to purchase
    pub select_index: Option<Vec<usize>>,
    #[structopt(long)]
    /// To report the result but not actually make a purchase
    pub dry_run: bool,
    #[structopt(short, long)]
    /// If we should use `chromedriver` instead of `geckodriver`
    pub chrome: bool,
}

#[derive(Debug)]
struct AccountInfo {
    username: String,
    password: String,
}

impl FromStr for AccountInfo {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(":");
        let username = if let Some(username) = parts.next() {
            username.to_string()
        } else {
            return Err("Invalid account info value expected <username>:<password>");
        };
        let password = if let Some(password) = parts.next() {
            password.to_string()
        } else {
            return Err("Invalid account info value expected <username>:<password>");
        };
        Ok(Self { username, password })
    }
}

#[tokio::main]
async fn main() -> R<()> {
    let opts = Opts::from_args();
    let mut _driver = if opts.chrome {
        tokio::process::Command::new("chromedriver")
            .arg("--port=4444")
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?
    } else {
        tokio::process::Command::new("geckodriver")
            .arg("--port")
            .arg("4444")
            .kill_on_drop(true)
            .spawn()?
    };
    if let Some(std_out) = _driver.stdout.as_mut() {
        let mut buf = [0u8;256];
        let _ = std_out.read(&mut buf).await?;
        println!("out: {:?}", String::from_utf8_lossy(&buf));
    } else {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    let mut tab = ClientBuilder::native()
        .connect("http://localhost:4444")
        .await?;
    tab.goto("https://www.titan.fitness/login").await?;
    tab.find_and_type("#login-form-email", &opts.account_info.username)
        .await?;
    tab.find_and_type("#login-form-password", &opts.account_info.password)
        .await?;
    find_and_click(&mut tab, ".login > .btn").await?;
    tab.wait_query(".my-account-main", 5000).await?;
    tab.goto("https://www.titan.fitness/cart").await?;
    if let Ok(eles) = tab.query_selector_all(".remove-product").await {
        for ele in eles {
            ele.click().await?;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let confirm = tab.query_selector(".cart-delete-confirmation-btn").await?;
            confirm.click().await?;
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
    tab.goto(&opts.url).await?;

    if let Some(idxes) = opts.select_index {
        if let Ok(mut selects) = tab.query_selector_all(".attribute-row select").await {
            for (&idx, select) in idxes.iter().zip(selects.iter_mut()) {
                select.clone().select_by_index(idx).await?;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    let mut ele = tab.wait_for_find(Locator::Css(".add-to-cart")).await?;
    if ele.attr("disabled").await?.is_some() {
        println!("item not available");
        tab.close().await.unwrap();
        _driver.kill().await.unwrap();
        return Ok(());
    }
    ele.click().await?;
    tab.goto("https://www.titan.fitness/startcheckout").await?;
    tab.find_and_type("#email", "r.f.masen@gmail.com").await?;
    tab.find_and_type("#shippingFirstName", "Robert").await?;
    tab.find_and_type("#shippingLastName", "Masen").await?;
    tab.find_and_type("#shippingAddressOne", "1201 Buchanan St NE")
        .await?;
    tab.find_and_type("#shippingAddressCity", "Minneapolis")
        .await?;
    tab.find_and_select("#shippingState", "MN").await?;
    tab.find_and_type("#shippingZipCode", "55413").await?;
    tab.find_and_type("#shippingPhoneNumber", "2487872339")
        .await?;
    ensure_no_subscribe(&mut tab).await?;
    tab.find_and_click(".submit-shipping").await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    tab.find_and_type("#saved-payment-security-code", "502")
        .await?;
    tab.find_and_click(".submit-payment").await?;
    if let Some(sub_total) = ensure_subtotal(&mut tab, opts.price).await? {
        println!(
            "Subtotal too large for purchase sub total: {} target: {}",
            sub_total, opts.price
        );
    } else {
        if opts.dry_run {
            println!("Would have purchased!")
        } else {
            find_and_click(&mut tab, ".place-order-btn").await?;
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    tab.close().await.unwrap();
    _driver.kill().await.unwrap();
    Ok(())
}

async fn find_and_click(
    tab: &mut fantoccini::Client,
    selector: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let ele = tab.query_selector(selector).await?;
    ele.click().await?;
    Ok(())
}

async fn ensure_no_subscribe(tab: &mut fantoccini::Client) -> R<()> {
    let mut ele = tab.query_selector("#newsletterSubscribeCheck").await?;
    let is_checked = ele.attr("checked").await?.is_some();
    if is_checked {
        let label = tab
            .find(Locator::Css(r#"label[for="newsletterSubscribeCheck"]"#))
            .await?;
        label.click().await?;
    }
    Ok(())
}

async fn ensure_subtotal(tab: &mut fantoccini::Client, value: f32) -> R<Option<f32>> {
    let mut ele = tab.query_selector(".sub-total").await?;
    let inner = ele.html(true).await?;
    let sub: f32 = inner.trim().trim_start_matches('$').parse()?;
    if sub <= value {
        Ok(None)
    } else {
        Ok(Some(sub))
    }
}

#[async_trait::async_trait]
trait ClientExt {
    async fn query_selector(
        &mut self,
        selector: &str,
    ) -> Result<Element, fantoccini::error::CmdError>;
    async fn wait_query(
        &mut self,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<Option<Element>, fantoccini::error::CmdError>;
    async fn query_selector_all(
        &mut self,
        selector: &str,
    ) -> Result<Vec<Element>, fantoccini::error::CmdError>;
    async fn find_and_type(
        &mut self,
        selector: &str,
        text: &str,
    ) -> Result<(), fantoccini::error::CmdError>;
    async fn find_and_click(&mut self, selector: &str) -> Result<(), fantoccini::error::CmdError>;
    async fn find_and_select(
        &mut self,
        selector: &str,
        value: &str,
    ) -> Result<(), fantoccini::error::CmdError>;
}
#[async_trait::async_trait]
impl ClientExt for fantoccini::Client {
    async fn query_selector(
        &mut self,
        selector: &str,
    ) -> Result<Element, fantoccini::error::CmdError> {
        self.find(Locator::Css(selector)).await
    }
    async fn wait_query(
        &mut self,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<Option<Element>, fantoccini::error::CmdError> {
        if let Ok(res) = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            self.wait_for_find(Locator::Css(selector)),
        )
        .await
        {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }
    async fn query_selector_all(
        &mut self,
        selector: &str,
    ) -> Result<Vec<Element>, fantoccini::error::CmdError> {
        self.find_all(Locator::Css(selector)).await
    }
    async fn find_and_type(
        &mut self,
        selector: &str,
        text: &str,
    ) -> Result<(), fantoccini::error::CmdError> {
        let mut ele = self.query_selector(selector).await?;
        ele.clear().await?;
        ele.send_keys(text).await?;
        Ok(())
    }
    async fn find_and_click(&mut self, selector: &str) -> Result<(), fantoccini::error::CmdError> {
        let ele = self.query_selector(selector).await?;
        ele.click().await?;
        Ok(())
    }
    async fn find_and_select(
        &mut self,
        selector: &str,
        value: &str,
    ) -> Result<(), fantoccini::error::CmdError> {
        let ele = self.query_selector(selector).await?;
        ele.select_by_value(value).await?;
        Ok(())
    }
}
#[async_trait::async_trait]
impl ClientExt for Element {
    async fn query_selector(
        &mut self,
        selector: &str,
    ) -> Result<Element, fantoccini::error::CmdError> {
        self.find(Locator::Css(selector)).await
    }
    async fn wait_query(
        &mut self,
        selector: &str,
        timeout_ms: u64,
    ) -> Result<Option<Element>, fantoccini::error::CmdError> {
        for _ in 0..10 {
            if let Ok(el) = self.query_selector(selector).await {
                return Ok(Some(el));
            }
            tokio::time::sleep(std::time::Duration::from_millis(timeout_ms / 10)).await;
        }
        Ok(None)
    }
    async fn query_selector_all(
        &mut self,
        selector: &str,
    ) -> Result<Vec<Element>, fantoccini::error::CmdError> {
        self.find_all(Locator::Css(selector)).await
    }
    async fn find_and_type(
        &mut self,
        selector: &str,
        text: &str,
    ) -> Result<(), fantoccini::error::CmdError> {
        let mut ele = self.query_selector(selector).await?;
        ele.send_keys(text).await?;
        Ok(())
    }
    async fn find_and_click(&mut self, selector: &str) -> Result<(), fantoccini::error::CmdError> {
        let ele = self.query_selector(selector).await?;
        ele.click().await?;
        Ok(())
    }
    async fn find_and_select(
        &mut self,
        selector: &str,
        value: &str,
    ) -> Result<(), fantoccini::error::CmdError> {
        let ele = self.query_selector(selector).await?;
        ele.select_by_value(value).await?;
        Ok(())
    }
}
