//! Strongly-typed enums mirroring `sdk/go/enums.go`.
//!
//! Every enum serializes to its lowercase wire-format string via `serde(rename_all = ...)`.

use serde::{Deserialize, Serialize};

/// Simulated vision deficiency for the screenshot API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VisionDeficiencyType {
    /// No simulated deficiency.
    None,
    /// Red-green color blindness (missing green cones).
    Deuteranopia,
    /// Red-green color blindness (missing red cones).
    Protanopia,
    /// Blue-yellow color blindness.
    Tritanopia,
    /// Total color blindness.
    Achromatopsia,
    /// Blurred vision simulation.
    BlurredVision,
    /// Reduced contrast simulation.
    ReducedContrast,
}

impl VisionDeficiencyType {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Deuteranopia => "deuteranopia",
            Self::Protanopia => "protanopia",
            Self::Tritanopia => "tritanopia",
            Self::Achromatopsia => "achromatopsia",
            Self::BlurredVision => "blurredVision",
            Self::ReducedContrast => "reducedContrast",
        }
    }
}

/// AI extraction model catalog.
///
/// See <https://scrapfly.io/docs/extraction-api/automatic-ai#models>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionModel {
    /// Article content.
    Article,
    /// Event listing.
    Event,
    /// Food recipe.
    FoodRecipe,
    /// Hotel page.
    Hotel,
    /// Hotel listing page.
    HotelListing,
    /// Job listing.
    JobListing,
    /// Job posting.
    JobPosting,
    /// Organization.
    Organization,
    /// Product page.
    Product,
    /// Product listing.
    ProductListing,
    /// Real-estate property.
    RealEstateProperty,
    /// Real-estate property listing.
    RealEstatePropertyListing,
    /// Review list.
    ReviewList,
    /// Search engine results page.
    SearchEngineResults,
    /// Social media post.
    SocialMediaPost,
    /// Software listing.
    Software,
    /// Stock quote.
    Stock,
    /// Vehicle ad.
    VehicleAd,
    /// Vehicle ad listing.
    VehicleAdListing,
}

impl ExtractionModel {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Event => "event",
            Self::FoodRecipe => "food_recipe",
            Self::Hotel => "hotel",
            Self::HotelListing => "hotel_listing",
            Self::JobListing => "job_listing",
            Self::JobPosting => "job_posting",
            Self::Organization => "organization",
            Self::Product => "product",
            Self::ProductListing => "product_listing",
            Self::RealEstateProperty => "real_estate_property",
            Self::RealEstatePropertyListing => "real_estate_property_listing",
            Self::ReviewList => "review_list",
            Self::SearchEngineResults => "search_engine_results",
            Self::SocialMediaPost => "social_media_post",
            Self::Software => "software",
            Self::Stock => "stock",
            Self::VehicleAd => "vehicle_ad",
            Self::VehicleAdListing => "vehicle_ad_listing",
        }
    }
}

/// Proxy pool catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyPool {
    /// Data-center proxies (cheaper, faster, easier to detect).
    PublicDatacenterPool,
    /// Residential proxies (more expensive, harder to detect).
    PublicResidentialPool,
}

impl ProxyPool {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PublicDatacenterPool => "public_datacenter_pool",
            Self::PublicResidentialPool => "public_residential_pool",
        }
    }
}

/// Screenshot capture flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotFlag {
    /// Load images (otherwise blocked for performance).
    LoadImages,
    /// Render in dark mode.
    DarkMode,
    /// Block cookie/consent banners.
    BlockBanners,
    /// Use print-media CSS.
    PrintMediaFormat,
    /// Request higher-quality output.
    HighQuality,
}

impl ScreenshotFlag {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoadImages => "load_images",
            Self::DarkMode => "dark_mode",
            Self::BlockBanners => "block_banners",
            Self::PrintMediaFormat => "print_media_format",
            Self::HighQuality => "high_quality",
        }
    }
}

/// Screenshot image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    /// JPEG output.
    Jpg,
    /// PNG output.
    Png,
    /// WebP output.
    Webp,
    /// GIF output.
    Gif,
}

impl ScreenshotFormat {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Gif => "gif",
        }
    }
}

/// Screenshot legacy option flags (distinct from `ScreenshotFlag` which
/// applies to the inline screenshots on a scrape).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotOption {
    /// Load images.
    LoadImages,
    /// Render in dark mode.
    DarkMode,
    /// Block cookie banners.
    BlockBanners,
    /// Print-media CSS.
    PrintMediaFormat,
}

impl ScreenshotOption {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoadImages => "load_images",
            Self::DarkMode => "dark_mode",
            Self::BlockBanners => "block_banners",
            Self::PrintMediaFormat => "print_media_format",
        }
    }
}

/// Scrape output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Structured JSON.
    Json,
    /// Plain text (HTML stripped).
    Text,
    /// Markdown.
    Markdown,
    /// Cleaned/normalized HTML.
    CleanHtml,
    /// Raw HTML.
    Raw,
}

impl Format {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::Markdown => "markdown",
            Self::CleanHtml => "clean_html",
            Self::Raw => "raw",
        }
    }
}

/// Additional options combinable with [`Format`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatOption {
    /// Strip links.
    NoLinks,
    /// Strip images.
    NoImages,
    /// Keep only the main content.
    OnlyContent,
}

impl FormatOption {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoLinks => "no_links",
            Self::NoImages => "no_images",
            Self::OnlyContent => "only_content",
        }
    }
}

/// HTTP methods the scrape endpoint accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `OPTIONS`.
    Options,
    /// `HEAD`.
    Head,
}

impl HttpMethod {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
        }
    }
}

/// Document-body compression format for the extraction endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionFormat {
    /// gzip.
    Gzip,
    /// zstd.
    Zstd,
    /// deflate.
    Deflate,
}

impl CompressionFormat {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Zstd => "zstd",
            Self::Deflate => "deflate",
        }
    }
}

/// Content formats the crawler API supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerContentFormat {
    /// Raw HTML.
    Html,
    /// Cleaned HTML.
    CleanHtml,
    /// Markdown.
    Markdown,
    /// Plain text.
    Text,
    /// Structured JSON.
    Json,
    /// Extracted data (AI template).
    ExtractedData,
    /// Page metadata.
    PageMetadata,
}

impl CrawlerContentFormat {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::CleanHtml => "clean_html",
            Self::Markdown => "markdown",
            Self::Text => "text",
            Self::Json => "json",
            Self::ExtractedData => "extracted_data",
            Self::PageMetadata => "page_metadata",
        }
    }
}

/// Crawler webhook events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerWebhookEvent {
    /// `crawler_started`.
    CrawlerStarted,
    /// `crawler_url_visited`.
    CrawlerUrlVisited,
    /// `crawler_url_skipped`.
    CrawlerUrlSkipped,
    /// `crawler_url_discovered`.
    CrawlerUrlDiscovered,
    /// `crawler_url_failed`.
    CrawlerUrlFailed,
    /// `crawler_stopped`.
    CrawlerStopped,
    /// `crawler_cancelled`.
    CrawlerCancelled,
    /// `crawler_finished`.
    CrawlerFinished,
}

impl CrawlerWebhookEvent {
    /// Wire-format string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CrawlerStarted => "crawler_started",
            Self::CrawlerUrlVisited => "crawler_url_visited",
            Self::CrawlerUrlSkipped => "crawler_url_skipped",
            Self::CrawlerUrlDiscovered => "crawler_url_discovered",
            Self::CrawlerUrlFailed => "crawler_url_failed",
            Self::CrawlerStopped => "crawler_stopped",
            Self::CrawlerCancelled => "crawler_cancelled",
            Self::CrawlerFinished => "crawler_finished",
        }
    }
}
