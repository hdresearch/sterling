# Vers configuration
The configuration algorithm used by vers executables is designed to be simple, introspectable, auditable, and easy to extend. For each configuration key required, the executable will first retrieve an environment variable with the given key. If not found, the executable will then default to its "sources" list, one of which must contain the key.  

The sources list has a predictable override order using the "source priority." For local sources (`.ini` files in the `config` directory), the source name is the file name with the trailing `.ini` trimmed, and the source priority refers to an integer value preceding a `-` character, for example: `config/500-common.ini` has priority `500` and name `500-common`. For remote sources, the source name is defined by the directive (See Section B) and the priority is extracted the same way. The end result is that a secret with name `600-secrets` will override the default `5xx-(name)` sources you see in this directory, and `500-common` will override a hypothetical `400-fallback` source, regardless of whether each source is local or remote.  

This allows for a flexible system able to aggregate multiple configuration sources, including environment variable overrides.  

It is required that the system record from which source a particular value is drawn. It is also required that the system reports any "unused" variables - variables defined by one or more sources (environment variables are not a source) that are unused by the application. The finalized config struct must be observable via a REST API endpoint. Runtime mutation of the struct is planned, but intentionally deferred until later to manage complexity. At present, changes to one or more sources will not be read until the next time the executable starts.

## Section A. Algorithm
On starting up, each vers executable will do the following:
1) If the `/etc/vers` directory or any required file isn't found, an error will be thrown.
2) Parse the `sources.txt` file (see Section B) and load the respective sources into memory.
3) Scan the `config` directory for ini files. Any file named `{priority}-{description}.ini` will be regarded as a source with source name `{priority}-{description}`. `{priority}` must be a decimal number. Any `.ini` file which does not conform to this naming convention will cause an error to be returned.
4) All sources are sorted by their `priority` in ascending order, such that `[10-base, 50-debug, 100-secret, 200-override]` will become `[10-base, 50-debug, 100-secret, 200-override]`. Note that the sort uses integer comparison, not string comparison. If any two sources have the same priority, an error will be returned.
5) For each source in the sorted list, the key-value pairs will be registered into a dictionary, overwriting the previous value if the key exists. (In other words, `50-debug` will overwrite `10-base` if the same key is contained in each.) Note that this means that an environment variable does not permit a required key to be omitted; at this time, environment-only configuration variables are not permitted.
6) For each key in the dictionary, if the same key (in `lower_snake_case`, as the ini is case-folded to lowercase at parse time) is present as an environment variable, the value from environment will override, and the source will be considered `environment`, with a functional `priority` of infinity.
7) A strongly-typed struct will be initialized from the dictionary. A field that lacks a corresponding dictionary entry will be regarded as an error unless the type is specified to be `Option<T>`. There are no "default" values permitted. Failure to initialize the struct will result in an immediate panic.
8) Any key in the dictionary which is not consumed by the struct will be reported with a warning. In other words, if one or more sources declare a configuration key-value pair which is not expected by the executable, those keys will be reported.

## Section B. sources.txt
The `sources.txt` file specifies sources for remote config files. The syntax is a newline-separated list of directives which will be used to load a remote source into memory. The following directives are supported:
- `aws-secret {source-name} {secret-id}`: Read the value of a secret with id `{secret-id}` from AWS Secret Manager, treating it as a source named `source-name`.
Note that the remote identifier, such as `aws-secret`'s `secret-id`, need not conform to the `source-name` format. `source-name`, however, must. Any source which does not will cause an error to be returned.

## Section C. INI dialect
The INI dialect used to read sources is defined as follows:
- Lines beginning with `#` are regarded as comments and ignored
- Lines begining with `[` are regarded as sections; these may be used in the future but are currently ignored
- Empty lines are ignored
- Non-empty lines must be of the format `key=value`, where `key` must conform to the regexp `/[a-zA-Z0-9_]+/`. `value` will be treated as a (UTF-8) string. Note that leading and trailing whitespace for both `key` and `value` are ignored.
- `key`s will be case-folded to lowercase at parse time.
- No escape sequences are acknowledged; this means that at present, no values may contain newlines etc.