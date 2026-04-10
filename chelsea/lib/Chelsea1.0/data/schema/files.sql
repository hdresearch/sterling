--
-- files.sql --
--
-- File System Manifest Database Schema
--
-------------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS Files(
  Id BLOB(16) NOT NULL,             -- Unique file identifier (e.g. GUID).
  Sequence INTEGER NOT NULL,        -- Process exactly in this order (ASC).
  Description TEXT NULL,            -- Human-readable description of this file.
  Type TEXT NULL,                   -- Type of 'special' file, e.g. 'symlink',
                                    -- etc.  If this value is NULL, the file is
                                    -- a 'normal' file.
  TargetId BLOB(16) NULL,           -- If the Type is 'SymbolicLink', this is
                                    -- the unique identifier for the target row
                                    -- of that symbolic link.
  Path TEXT NULL,                   -- Full parent path of the file.  If this
                                    -- value is NULL, the 'root path' for the
                                    -- installation will be used.
  Name TEXT NOT NULL,               -- Name and extension of the file.
  Owner TEXT NULL,                  -- POSIX-style owner for the file, with the
                                    -- user and group delimited by colon, e.g.
                                    -- "root:root" for POSIX -OR- a Security
                                    -- Identity (SID) for Windows.  If this
                                    -- value is NULL, the file will use the
                                    -- default owner.
  Permissions INTEGER NULL,         -- POSIX-style permissions for the file.
                                    -- If this value is NULL, the file will use
                                    -- the default permissions.  For Windows,
                                    -- this value may undergo a 'translation'
                                    -- process that maps these permissions to
                                    -- a suitable ACL (SDDL) string.
  Modified DATETIME NULL,           -- Last modified date/time in UTC as the
                                    -- number of seconds since the POSIX epoch.
                                    -- If this value is NULL, the file will use
                                    -- the default last modification time.
  Content BLOB NOT NULL,            -- The raw file bytes.
  HashAlgorithm NULL,               -- This is the cryptographic hash algorithm
                                    -- used when verifying the signature.  This
                                    -- should almost always be SHA512, at least
                                    -- until something better comes along.  If
                                    -- this value is NULL, a suitable default
                                    -- will be used.
  SignatureAlgorithm TEXT NULL,     -- Algorithm for signing, e.g. 'RSA-PSS'.
                                    -- If this value is NULL, a suitable
                                    -- default will be used.
  PublicKeyToken BLOB(8) NULL,      -- SNK pub token, e.g. 0x8bf43b4749e46a0b.
                                    -- If this value is NULL, a suitable
                                    -- default will be used.
  Signature BLOB NOT NULL,          -- Binary (RSA?) signature of the raw file
                                    -- content in this row (i.e. the 'Content'
                                    -- column).
  UNIQUE (Id),                      -- Id column must be unique.
  UNIQUE (Sequence),                -- Sequence column must be unique.
  UNIQUE (Path, Name),              -- Fully qualified path must be unique.
  CONSTRAINT targetId_fk1 FOREIGN KEY (
    TargetId
  ) REFERENCES Files(Id),
  CONSTRAINT type_ck1 CHECK (
    Type IS NULL OR Type = 'SymbolicLink'
  ),
  CONSTRAINT path_ck1 CHECK (
    Path IS NULL OR
    Path REGEXP '^(([\x25\x2D\._0-9A-Za-z]+)\x2F)*([\x25\x2D\._0-9A-Za-z]+)$'
  ),
  CONSTRAINT name_ck1 CHECK (
    Name REGEXP '^[\x2D_0-9A-Za-z]+(\.[\x2D_0-9A-Za-z]+)?$'
  ),
  CONSTRAINT owner_ck1 CHECK (
    Owner IS NULL OR
    Owner REGEXP '^[A-Za-z][\x2D0-9A-Za-z]*(:[A-Za-z][\x2D0-9A-Za-z]*)?$' OR
    Owner REGEXP '^S-(0|[1-9]\d*)-(0|[1-9]\d{0,14}|0x[0-9A-Fa-f]{1,12})(-(0|[1-9]\d{0,9})){1,15}$'
  ),
  CONSTRAINT permissions_ck1 CHECK (
    Permissions IS NULL OR
    Permissions BETWEEN 0 AND 0777
  ),
  CONSTRAINT hashAlgorithm_ck1 CHECK (
    HashAlgorithm IS NULL OR
    HashAlgorithm == 'SHA512'
  ),
  CONSTRAINT signatureAlgorithm_ck1 CHECK (
    SignatureAlgorithm IS NULL OR
    SignatureAlgorithm == 'RSA' OR
    SignatureAlgorithm == 'RSA-PSS'
  )
);
