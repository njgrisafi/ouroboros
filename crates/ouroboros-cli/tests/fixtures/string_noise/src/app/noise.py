# Dotted string that matches no first-party module. With
# --include-string-imports it is a candidate, but must be dropped
# silently -- it is not an unresolved import and must not trip --strict.
X = "no.such.module"
