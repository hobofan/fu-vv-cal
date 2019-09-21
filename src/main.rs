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
use snafu::{ensure, Backtrace, ErrorCompat, ResultExt, Snafu};
use timespan::{DateTimeSpan, NaiveDateTimeSpan};

type StdError = Box<dyn std::error::Error>;

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("The HTTP request for the course page was not successful"))]
    HttpRequestError,
}

// TODO: RELATED-TO to cancel all events of a series

/// Parse timespan of "Mo, 21.10.2019 10:00 - 13:00"
fn parse_timespan(date_text: String) -> Result<DateTimeSpan<chrono_tz::Tz>, StdError> {
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

#[derive(Debug, Clone)]
struct Course {
    name: String,
    events: Vec<CourseEvent>,
}

impl Course {
    pub fn from_document(document: &Document) -> Result<Self, StdError> {
        Ok(Self {
            name: Self::name_from_document(&document)?,
            events: CourseEvent::all_from_document(document)?,
        })
    }

    fn name_from_document(document: &Document) -> Result<String, StdError> {
        let node = document
            .find(Class("subc").descendant(Name("h1")))
            .next()
            .expect("Course has no name/title");

        Ok(node.text().trim().to_owned())
    }

    pub fn to_ical(self) -> Result<ICalendar<'static>, StdError> {
        let mut calendar = ICalendar::new("2.0", "ics-rs");

        let first_id = self.events.iter().next().unwrap().id.clone();
        for event in self.events.into_iter() {
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
            let mut cal_event = Event::new(event.id, start_date.to_string());
            cal_event.push(DtStart::new(start_date));
            cal_event.push(DtEnd::new(end_date));
            cal_event.push(Summary::new(self.name.clone()));
            cal_event.push(RelatedTo::new(first_id.clone()));
            cal_event.push(ics::components::Property::new("RELTYPE", "CHILD"));

            calendar.add_event(cal_event);
        }

        Ok(calendar)
    }
}

#[derive(Debug, Clone)]
struct CourseEvent {
    id: String,
    timespan: DateTimeSpan<chrono_tz::Tz>,
}

impl CourseEvent {
    pub fn all_from_document(document: &Document) -> Result<Vec<Self>, StdError> {
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

#[derive(Debug)]
struct RequestedCourse {
    pub id: String,
    pub semester: String,
}

impl RequestedCourse {
    pub fn new<S1: Into<String>, S2: Into<String>>(id: S1, semester: S2) -> Self {
        Self {
            id: id.into(),
            semester: semester.into(),
        }
    }

    pub async fn get_as_ical<'a>(&self) -> Result<ICalendar<'a>, StdError> {
        let body_str = self.request_course().await?;

        let document = Document::from(body_str.as_str());
        let course = Course::from_document(&document)?;

        course.to_ical()
    }

    pub async fn save_as_ical<'a, P: Into<std::path::PathBuf>>(
        &self,
        path: P,
    ) -> Result<(), StdError> {
        let calendar = self.get_as_ical().await?;
        calendar.save_file(path.into())?;
        Ok(())
    }

    async fn request_course(&self) -> Result<String, StdError> {
        let https = HttpsConnector::new().unwrap();
        let client = Client::builder().build::<_, hyper::Body>(https);

        let url = format!(
            "https://www.fu-berlin.de/vv/de/lv/{id}?sm={semester}",
            id = self.id,
            semester = self.semester
        );
        let res = client.get(url.parse().unwrap()).await?;
        let status = res.status();
        let mut body = res.into_body();
        let mut bytes = Vec::new();
        while let Some(next) = body.next().await {
            let chunk = next.unwrap();
            bytes.extend(chunk);
        }
        let body_str = String::from_utf8(bytes).unwrap();

        if !status.is_success() {
            dbg!(&self.id);
            return Err(Error::HttpRequestError.into());
        }

        Ok(body_str)
    }
}

#[tokio::main]
async fn main() -> Result<(), StdError> {
    // OC 1 Vorlesung
    let course = RequestedCourse::new("524870", "498562");
    course.save_as_ical("oc1_vorlesung.ics").await?;
    // OC1 Uebungen
    let course = RequestedCourse::new("524871", "498562");
    course.save_as_ical("oc1_uebung.ics").await?;
    // BC 1 Vorlesung
    let course = RequestedCourse::new("525101", "498562");
    course.save_as_ical("bc1_vorlesung.ics").await?;
    // BC1 Uebungen
    let course = RequestedCourse::new("525102", "498562");
    course.save_as_ical("bc1_uebung.ics").await?;
    // Botanik Vorlesung
    let course = RequestedCourse::new("503925", "498562");
    course.save_as_ical("botanik_vorlesung.ics").await?;
    // Botanik Seminar A
    let course = RequestedCourse::new("503926", "498562");
    course.save_as_ical("botanik_seminar_a.ics").await?;
    // Botanik Seminar B
    let course = RequestedCourse::new("503927", "498562");
    course.save_as_ical("botanik_seminar_b.ics").await?;

    Ok(())
}
