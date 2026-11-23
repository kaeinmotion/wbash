1 ok, 1 error, 2 total
#1 read ok
src/main.rs (5 lines, b3:aaa38e2c80a5b5f6)
1: mod config;
2:
3: fn main() {
4:     let _ = config::parse_config("x");
5: }
#2 read error
!not_found 'nope.rs' does not exist. use `w tree` or `w search` to locate it
