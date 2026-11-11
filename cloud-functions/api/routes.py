"""Restore the /api prefix that EdgeOne removes before Flask sees the request."""


def editor_target(path, query):
    if not path.startswith("/"):
        path = "/" + path
    target = "/api" if path == "/" else "/api" + path
    if query:
        target += "?" + query
    return target
