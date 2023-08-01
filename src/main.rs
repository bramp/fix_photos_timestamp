use std::fmt::Formatter;
use std::fmt::Display;
use chrono::DurationRound;
use chrono::Duration;
use async_stream::stream;
use chrono::NaiveDate;
use chrono::NaiveDateTime;
use chrono::NaiveTime;
use chrono::TimeZone;
use chrono_tz::Tz;
use futures_core::Stream;
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
use google_photoslibrary1::api::MediaItem;
use google_photoslibrary1::api::MediaMetadata;
use google_photoslibrary1::FieldMask;
use std::num::ParseIntError;
use std::str::FromStr;
extern crate google_photoslibrary1 as photoslibrary1;
use chrono::offset::Utc;
use chrono::DateTime;
use google_photoslibrary1::api::SearchMediaItemsRequest;
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use lazy_static::lazy_static;
use photoslibrary1::Error;
use photoslibrary1::PhotosLibrary;
use regex::Regex;
use std::env;

// Range in the format [begin, end).
#[derive(Debug)]
struct DateTimeRange<Tz: chrono::TimeZone> {
    begin: DateTime<Tz>,
    end: DateTime<Tz>,
}

impl DateTimeRange<Tz> {
    fn contains<Tz2: TimeZone>(&self, d: &DateTime<Tz2>) -> bool {
        d >= &self.begin && d < &self.end
    }

    fn timezone(&self) -> Tz {
        return self.begin.timezone();
    }
}

impl Display for DateTimeRange<Tz> {
    fn fmt(&self, out: &mut Formatter<'_>) -> Result<(), std::fmt::Error> { 
        write!(out, "[{} - {})", self.begin, self.end)
    }
}

// TODO It would be nice to templatise the HttpsConnector
async fn list_albums(hub: &PhotosLibrary<HttpsConnector<HttpConnector>>) {
    let mut next_page_token = String::new();
    loop {
        // You can configure optional parameters by calling the respective setters at will, and
        // execute the final call using `doit()`.
        // Values shown here are possibly random and not representative !
        let result = hub
            .albums()
            .list()
            .page_token(&next_page_token) // I do not like that this starts as "", it should be None
            .page_size(50)
            .exclude_non_app_created_data(false)
            .doit()
            .await;

        let result = match result {
            Err(e) => match e {
                // The Error enum provides details about what exactly happened.
                // You can also just use its `Debug`, `Display` or `Error` traits
                Error::HttpError(_)
                | Error::Io(_)
                | Error::MissingAPIKey
                | Error::MissingToken(_)
                | Error::Cancelled
                | Error::UploadSizeLimitExceeded(_, _)
                | Error::Failure(_)
                | Error::BadRequest(_)
                | Error::FieldClash(_)
                | Error::JsonDecodeError(_, _) => panic!("{}", e),
            },
            Ok(res) => res.1,
        };

        for album in result.albums.unwrap_or_default() {
            println!(
                "{:>76} {:}",
                album.id.unwrap_or_default(),
                album.title.unwrap_or("unnamed".to_string())
            );
        }

        next_page_token = match result.next_page_token {
            Some(token) => token,
            None => break,
        }
    }
}

fn list_media<'a>(
    hub: &'a PhotosLibrary<HttpsConnector<HttpConnector>>,
    album_id: &'a str,
) -> impl Stream<Item = MediaItem> + 'a {
    stream! {
        let mut next_page_token = None;
        loop {
            let result = hub
                .media_items()
                .search(SearchMediaItemsRequest {
                    album_id: Some(album_id.to_string()),
                    page_size: Some(10),
                    page_token: next_page_token,
                    ..Default::default()
                })
                .doit()
                .await;

            let result = match result {
                Err(e) => panic!("{:?}", e),
                Ok(res) => res.1,
            };

            let photos = result.media_items;
            for photo in photos.unwrap_or_default() {
                yield photo;
            }

            if result.next_page_token.is_none() {
                break;
            }
            next_page_token = result.next_page_token
        }
    }
}

enum ProcessResult {
    Ok,                     // nothing to change
    Suggest(DateTime<Utc>), // should change to
    Unknown,                // not sure what to change to

    Error(ParseIntError), // error
}

fn process_media(
    filename: &str,
    creation_time: &DateTime<Utc>,
    allowed_range: &DateTimeRange<Tz>,
) -> ProcessResult {

    let d = parse_date_from_filename(filename);

    if allowed_range.contains(&creation_time.with_timezone(&allowed_range.begin.timezone())) {
        if d.is_none() {
            // Creation timestamp looks good, and I have no more information
            return ProcessResult::Ok;
        }

        let d = d.unwrap();

        let creation_time = creation_time.duration_trunc(Duration::minutes(1)).unwrap();

        let d_utc = Utc.from_utc_datetime(&d);
        let d_utc = d_utc.duration_trunc(Duration::minutes(1)).unwrap();

        let d_local = allowed_range.timezone().from_local_datetime(&d).unwrap();
        let d_local = d_local.duration_trunc(Duration::minutes(1)).unwrap();

        if creation_time == d_utc || creation_time == d_local {
            return ProcessResult::Ok;
        }

        // otherwise drop down to try and figure out a good time
    }

    // Figure out the correct time
    if let Some(d) = parse_date_from_filename(filename) {
        let d_utc = Utc.from_utc_datetime(&d);
        if allowed_range.contains(&d_utc) {
            // Good to use this!
            return ProcessResult::Suggest(d_utc);
        }

        // Maybe assume the timezone is the same as the range
        let d_local = allowed_range.timezone().from_local_datetime(&d).unwrap();
        if allowed_range.contains(&d_local) {
            // Good to use this!
            return ProcessResult::Suggest(d_local.with_timezone(&Utc));
        }
    }

    ProcessResult::Unknown
}

lazy_static! {
    // This is one specific date format. TODO generalise this far better.
    static ref RE: Regex = Regex::new(r"([0-9]{4})([0-9]{2})([0-9]{2})[-_]([0-9]{2})([0-9]{2})([0-9]{2})").unwrap();
}

// Parse the filename and see if it has a timestamp in it.
// TODO change from NaiveDate to DateTime if we know the timezone.
fn parse_date_from_filename(filename: &str) -> Option<NaiveDateTime> {
    let captures = RE.captures(&filename);
    let (_full, [year, month, day, hour, minute, second]) = match captures {
        None => return None, // TODO something here
        Some(c) => c.extract(),
    };

    let d = NaiveDate::from_ymd_opt(
        year.parse::<i32>().unwrap(),
        month.parse::<u32>().unwrap(),
        day.parse::<u32>().unwrap(),
    )
    .unwrap();
    let t = NaiveTime::from_hms_milli_opt(
        hour.parse::<u32>().unwrap(),
        minute.parse::<u32>().unwrap(),
        second.parse::<u32>().unwrap(),
        0, /*TODO*/
    )
    .unwrap();

    Some(d.and_time(t))
}

// TODO The Google Photos API doesn't actually let you update things :(
async fn _update_media(
    hub: &PhotosLibrary<HttpsConnector<HttpConnector>>,
    media_id: &str,
    creation_time: DateTime<Utc>,
) {
    let req = MediaItem {
        media_metadata: Some(MediaMetadata {
            creation_time: Some(creation_time),
            ..Default::default()
        }),
        ..Default::default()
    };

    let result = hub
        .media_items()
        .patch(req, media_id)
        .update_mask(FieldMask::from_str("mediaMetadata.creationTime").unwrap())
        .doit()
        .await;

    if let Err(e) = result {
        panic!("Failed to update {}: {:?}", media_id, e);
    }
}

#[tokio::main]
async fn main() {
    // Get an ApplicationSecret instance by some means. It contains the `client_id` and
    // `client_secret`, among other things.
    // Get one from https://developers.google.com/photos/library/guides/get-started#enable-the-api
    let secret = yup_oauth2::read_application_secret("credentials.json")
        .await
        .expect("credentials.json");

    // Instantiate the authenticator. It will choose a suitable authentication flow for you,
    // unless you replace  `None` with the desired Flow.
    // Provide your own `AuthenticatorDelegate` to adjust the way it operates and get feedback about
    // what's going on. You probably want to bring in your own `TokenStorage` to persist tokens and
    // retrieve them from storage.
    let auth = yup_oauth2::InstalledFlowAuthenticator::builder(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
    )
    .persist_tokens_to_disk("tokencache.json")
    .build()
    .await
    .expect("failed to create authenticator");

    let scopes = &[
        // Read access to List Albums, and Search for Media in Albums
        "https://www.googleapis.com/auth/photoslibrary.readonly",
        // Edit access to update the timestamps
        "https://www.googleapis.com/auth/photoslibrary.edit.appcreateddata",
    ];

    if let Err(e) = auth.token(scopes).await {
        panic!("failed to get auth tokens: {:?}", e);
    }

    let hub = PhotosLibrary::new(
        hyper::Client::builder().build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .https_only()
                .enable_http1()
                .enable_http2()
                .build(),
        ),
        auth,
    );

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage {:} {{album_id}}", args[0]);
        println!();

        list_albums(&hub).await;

        return;
    }

    // TODO make this a command line argument
    let tz: Tz = "America/Los_Angeles".parse().unwrap();

    // All photos in this album should be within this date range.
    let allowed_range = DateTimeRange {
        //begin: tz.with_ymd_and_hms(2023, 07, 15, 08, 30, 00).unwrap(), // 15:30:00 UTC
        //end: tz.with_ymd_and_hms(2023, 07, 15, 12, 00, 00).unwrap(),   // 19:00:00 UTC

        begin: tz.with_ymd_and_hms(2023, 07, 30, 17, 00, 00).unwrap(), 
        end: tz.with_ymd_and_hms(2023, 07, 30, 19, 00, 00).unwrap(), 
    };

    println!("Valid range: {}", allowed_range);

    let album_id = &args[1];

    let media_stream = list_media(&hub, album_id);
    pin_mut!(media_stream); // needed for iteration

    let mut count = 0;
    while let Some(media) = media_stream.next().await {
        count += 1;

        let filename = media.filename.as_ref().unwrap();
        let creation_time = media
            .media_metadata
            .as_ref()
            .unwrap()
            .creation_time
            .unwrap();

        print!("{:3} {:} {:}", count, filename, creation_time);

        match process_media(&filename, &creation_time, &allowed_range) {
            ProcessResult::Ok => println!(" OK"),
            ProcessResult::Unknown => println!(" ¯\\_(ツ)_/¯"),
            ProcessResult::Suggest(d) => {
                println!(" change to {}", d);
                //update_media(&hub, &media.id.as_ref().unwrap(), d).await;
            }
            ProcessResult::Error(e) => println!(" {}", e),
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::parse_date_from_filename;
    use crate::NaiveDate;
    use crate::NaiveTime;
    use chrono::NaiveDateTime;

    fn naive_date(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> NaiveDateTime {
        let d = NaiveDate::from_ymd_opt(year, month, day).unwrap();
        let t = NaiveTime::from_hms_milli_opt(hour, minute, second, 0 /*TODO*/).unwrap();
        d.and_time(t)
    }

    #[test]
    fn test_parse_date_from_filename() {
        let tests = vec![
            // These are in local time (and correct)
            (
                "Screenshot_20230715_095113_CluedUpp Geogames.jpg",
                naive_date(2023, 07, 15, 09, 51, 13),
            ),
            /*
            // These two dates are missing the month :/
            (
                "cluedUpp_teamPhoto2023-15-9-54-28-746.png",
                naive_date(2023, 07, 15, 09, 54, 28),
            ), // and 746ms
            (
                "CluedUpp_AR_2023-15-10-6-43-804.png",
                naive_date(2023, 07, 15, 10, 06, 43),
            ), // and 804ms
            */
            // These are in UTC (need adjusting)
            (
                "PXL_20230715_170131447.jpg",
                naive_date(2023, 07, 15, 17, 01, 31),
            ), // and 447ms
            (
                "Screenshot_20230715-172906.png",
                naive_date(2023, 07, 15, 17, 29, 06),
            ),
            (
                "Screenshot_20230715-210645.png",
                naive_date(2023, 07, 15, 21, 06, 45),
            ),
        ];

        for (filename, expected) in tests {
            assert_eq!(
                parse_date_from_filename(filename),
                Some(expected),
                "filename: {}",
                filename
            );
        }
    }
}
