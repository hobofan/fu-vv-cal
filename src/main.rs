use chrono::TimeZone;
use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Europe::Berlin;
use hyper::Client;
use hyper_tls::HttpsConnector;
use ics::properties::{
    Categories, Description, DtEnd, DtStart, Organizer, RelatedTo, Status, Summary,
};
use ics::{escape_text, Event, ICalendar};
use select::document::Document;
use select::predicate::{Attr, Class, Name, Predicate};
use timespan::{DateTimeSpan, NaiveDateTimeSpan};

// TODO: RELATED-TO to cancel all events of a series

/// Parse timespan of "Mo, 21.10.2019 10:00 - 13:00"
fn parse_timespan(
    date_text: String,
) -> Result<DateTimeSpan<chrono_tz::Tz>, Box<dyn std::error::Error>> {
    let date_text = date_text[4..].to_owned();

    let date_split = date_text.split(" ").collect::<Vec<_>>();
    let date_day = date_split[0];
    let date_start_time = date_split[1];
    let date_end_time = date_split[3];

    let date_day = NaiveDate::parse_from_str(&date_day, "%d.%m.%Y")?;
    let date_start_time = NaiveTime::parse_from_str(&date_start_time, "%R")?;
    let date_end_time = NaiveTime::parse_from_str(&date_end_time, "%R")?;

    let start_date = date_day.and_time(date_start_time);
    let end_date = date_day.and_time(date_end_time);

    let date_span = DateTimeSpan::from_local_datetimespan(
        &NaiveDateTimeSpan::new(start_date, end_date).unwrap(),
        &Berlin,
    );

    date_span.map_err(Into::into)
}

#[derive(Debug)]
struct Course {
    name: String,
    events: Vec<CourseEvent>,
}

impl Course {
    pub fn from_document(document: &Document) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            name: Self::name_from_document(&document)?,
            events: CourseEvent::all_from_document(document)?,
        })
    }

    fn name_from_document(document: &Document) -> Result<String, Box<dyn std::error::Error>> {
        let node = document
            .find(Class("subc").descendant(Name("h1")))
            .next()
            .expect("Course has no name/title");

        Ok(node.text().trim().to_owned())
    }

    pub fn to_ical(&self) -> Result<ICalendar, Box<dyn std::error::Error>> {
        let mut calendar = ICalendar::new("2.0", "ics-rs");

        let first_id = self.events.iter().next().unwrap().id.clone();
        for event in self.events.iter() {
            let start_date = event
                .timespan
                .start
                .naive_utc()
                .format("%Y%m%dT%H%M%SZ")
                .to_string();
            let end_date = event
                .timespan
                .end
                .naive_utc()
                .format("%Y%m%dT%H%M%SZ")
                .to_string();
            let mut cal_event = Event::new(&event.id, start_date.to_string());
            cal_event.push(DtStart::new(start_date));
            cal_event.push(DtEnd::new(end_date));
            cal_event.push(Summary::new(&self.name));
            cal_event.push(RelatedTo::new(first_id.clone()));
            cal_event.push(ics::components::Property::new("RELTYPE", "CHILD"));

            calendar.add_event(cal_event);
        }

        Ok(calendar)
    }
}

#[derive(Debug)]
struct CourseEvent {
    id: String,
    timespan: DateTimeSpan<chrono_tz::Tz>,
}

impl CourseEvent {
    pub fn all_from_document(document: &Document) -> Result<Vec<Self>, Box<dyn std::error::Error>> {
        let mut events = vec![];
        for node in document.find(Class("link_to_details")) {
            let date_node = node.find(Class("course_date_time")).next().unwrap();
            let date_text = date_node.text().trim().to_owned();

            let date_span = parse_timespan(date_text)?;

            let id = node.attr("id").unwrap().replace("link_to_details_", "");

            events.push(CourseEvent {
                id,
                timespan: date_span,
            })
        }

        Ok(events)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let https = HttpsConnector::new().unwrap();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let res = client
        .get(
            "https://www.fu-berlin.de/vv/de/lv/524870?sm=498562"
                .parse()
                .unwrap(),
        )
        .await?;
    let status = res.status();
    let mut body = res.into_body();
    let mut bytes = Vec::new();
    while let Some(next) = body.next().await {
        let chunk = next.unwrap();
        bytes.extend(chunk);
    }
    let body_str = String::from_utf8(bytes).unwrap();

    if !status.is_success() {
        dbg!(&body_str);
    }

    let document = Document::from(body_str.as_str());
    let course = Course::from_document(&document)?;
    dbg!(&course);

    let calendar = course.to_ical()?;
    calendar.save_file("toasty.ics")?;

    Ok(())
}
