# Fix Google Photo Timestamps

by Andrew Brampton (https://bramp.net)

This is a failed attempt to fix Creation Timestamps on Google Photo images.

Specifically, I have files like "Screenshot_20230730-182922.png" that don't
have the correct creation time. (They seem to have the upload time). This tool
will parse a album, and print out all the files with the wrong timestamp, i.e,
the files which have times in the name, but the Google photos metadata does not
match.

This is a failed attempt, because Google Photos does not actually allow changing
the creation timestamp from the API :( You can manually fix it via the UI. So I
used this tool to ensure the new times were correct.

Usage:

```
# List all your albums
$ cargo run

Usage target/debug/fix_photos_timestamp {album_id}

AE3SYY.........kPJA  Album One Title
AE3SYY.......u_4mDQ  Album Two Title
AE3SYY.......oj16GT  Album Three Title
AE3SYY.....33t-16MJ  Album Four Title
...

# Now fix the specific album
$ cargo run -- AE3SYY.........kPJA

Valid range: [2023-07-30 17:00:00 PDT - 2023-07-30 19:00:00 PDT)

  1 Screenshot_20230730-171653.png 2023-07-31 00:16:00 UTC OK
  2 Screenshot_20230730-171634.png 2023-07-31 00:16:00 UTC OK
  3 Screenshot_20230730-171700.png 2023-07-31 00:17:00 UTC OK
  4 Screenshot_20230730-171712.png 2023-07-31 00:17:00 UTC OK
  5 Screenshot_20230730-172308.png 2023-07-31 00:23:08 UTC OK
  ...
 10 Screenshot_20230730-183124.png 2023-08-01 21:49:20 UTC change to 2023-07-31 01:31:24 UTC
 11 Screenshot_20230730-183122.png 2023-08-01 21:49:22 UTC change to 2023-07-31 01:31:22 UTC
 12 Screenshot_20230730-183010.png 2023-08-01 21:49:23 UTC change to 2023-07-31 01:30:10 UTC
```

Now go manually fix all the timestamps on https://photos.google.com/

# License: Apache-2.0

