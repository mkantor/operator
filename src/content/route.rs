use serde::Serialize;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("Invalid route '{}': {}", .route, .message)]
pub struct InvalidRouteError {
    route: String,
    message: String,
}

fn canonicalize_route(route: &str) -> Result<String, InvalidRouteError> {
    if !route.starts_with('/') {
        Err(InvalidRouteError {
            route: String::from(route),
            message: String::from("Routes must be absolute (start with a '/')"),
        })
    } else {
        let canonicalized_components = route.split('/').filter(|component| !component.is_empty());

        let canonicalized_route = canonicalized_components.collect::<Vec<&str>>().join("/");

        Ok(format!("/{canonicalized_route}"))
    }
}

/// A canonicalized absolute URI path.
#[derive(Debug, Clone, Hash, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Route(String);
impl FromStr for Route {
    type Err = InvalidRouteError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        canonicalize_route(input).map(Route)
    }
}
impl AsRef<str> for Route {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl fmt::Display for Route {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn valid_routes_succeed() {
        let result = "/foo/bar".parse::<Route>();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_ref(), "/foo/bar");
    }

    #[test]
    fn invalid_routes_fail() {
        assert!("no-leading-slash".parse::<Route>().is_err());
        assert!("".parse::<Route>().is_err());
    }

    #[test]
    fn canonicalization_produces_identical_routes() {
        let canonical_route = "/foo/bar".parse::<Route>().unwrap();
        let identical_routes = [
            "/foo/bar/",
            "//foo/bar/",
            "/foo/bar//",
            "/foo//bar",
            "////foo/////bar////",
        ];

        for input in identical_routes.iter() {
            let result = input.parse::<Route>();
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), canonical_route);
        }
    }

    #[test]
    fn root_route_can_exist() {
        let one_slash_result = "/".parse::<Route>();
        assert!(one_slash_result.is_ok());
        let one_slash_route = one_slash_result.unwrap();

        let buncha_slashes_result = "////".parse::<Route>();
        assert!(buncha_slashes_result.is_ok());
        let buncha_slashes_route = buncha_slashes_result.unwrap();

        assert_eq!(one_slash_route, buncha_slashes_route);
        assert_eq!(one_slash_route.as_ref(), "/");
        assert_eq!(buncha_slashes_route.as_ref(), "/");
    }
}
