//! Gallery of the color tokens in both normal variants and in high contrast, drawn from the
//! crate's own values (TS-CKP-004, 2026-10-05 amendment).
//!
//!   cargo run -p gitraptor-theme --example palette            truecolor swatches in the terminal
//!   cargo run -p gitraptor-theme --example palette -- --html  the same as an HTML fragment

use gitraptor_theme::{Background, ColorToken, Contrast, Rgb, Role, contrast_ratio};

/// A set to show: title, set, terminal background and the grounds it is shown on.
type Set = (
    &'static str,
    Contrast,
    Background,
    &'static [(&'static str, Rgb)],
);

/// The grounds each set is shown on: its typical terminals, or its own ground in high contrast.
const SETS: [Set; 3] = [
    (
        "Dark terminal",
        Contrast::Normal,
        Background::Dark,
        &[
            ("#1e1e1e", Rgb(0x1e, 0x1e, 0x1e)),
            ("Solarized dark", Rgb(0x00, 0x2b, 0x36)),
        ],
    ),
    (
        "Light terminal",
        Contrast::Normal,
        Background::Light,
        &[
            ("white", Rgb(0xff, 0xff, 0xff)),
            ("Solarized light", Rgb(0xfd, 0xf6, 0xe3)),
        ],
    ),
    (
        "High contrast",
        Contrast::High,
        Background::Dark,
        &[("own ground", Rgb(0, 0, 0))],
    ),
];

fn hex(Rgb(r, g, b): Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn main() {
    let html = std::env::args().any(|a| a == "--html");
    for (title, contrast, background, grounds) in SETS {
        let v = |t: ColorToken| t.values(contrast, background);
        let text = |ground: Rgb| match contrast {
            Contrast::High => v(ColorToken::TextDefault).rgb,
            // The normal sets inherit the terminal's text: show its reference value.
            Contrast::Normal => {
                if background == Background::Light {
                    v(ColorToken::TextDefault).rgb
                } else if ground == Rgb(0x00, 0x2b, 0x36) {
                    Rgb(0x93, 0xa1, 0xa1)
                } else {
                    Rgb(0xd4, 0xd4, 0xd4)
                }
            }
        };
        if html {
            println!("<h2>{title}</h2><div class=\"grounds\">");
        } else {
            println!("\n{title}");
        }
        for (name, ground) in grounds {
            let (Rgb(br, bg, bb), Rgb(tr, tg, tb)) = (*ground, text(*ground));
            if html {
                println!(
                    "<div class=\"term\" style=\"background:{};color:{}\"><div class=\"label\">{name}</div>",
                    hex(*ground),
                    hex(text(*ground))
                );
            } else {
                println!("\x1b[48;2;{br};{bg};{bb}m\x1b[38;2;{tr};{tg};{tb}m  {name:<56}\x1b[0m");
            }
            for token in ColorToken::ALL {
                let c = v(token).rgb;
                let Rgb(r, g, b) = c;
                let line = match token.role() {
                    Role::Background if token == ColorToken::BgDefault => continue,
                    Role::Background => {
                        let fg = text(*ground);
                        let ratio = contrast_ratio(fg, c);
                        if html {
                            format!(
                                "<div class=\"row\" style=\"background:{}\"><span>{}</span><span>{} · text {ratio:.1}</span></div>",
                                hex(c),
                                token.name(),
                                hex(c)
                            )
                        } else {
                            format!(
                                "\x1b[48;2;{r};{g};{b}m\x1b[38;2;{tr};{tg};{tb}m  {:<20} {}  text {ratio:>4.1}{:<19}\x1b[0m",
                                token.name(),
                                hex(c),
                                ""
                            )
                        }
                    }
                    Role::Foreground => {
                        let on = if token == ColorToken::TextInverse {
                            v(ColorToken::AccentDefault).rgb
                        } else {
                            *ground
                        };
                        let ratio = contrast_ratio(c, on);
                        let label = if token == ColorToken::TextInverse {
                            " (on accent)"
                        } else {
                            ""
                        };
                        if html {
                            let style = if token == ColorToken::TextInverse {
                                format!("color:{};background:{}", hex(c), hex(on))
                            } else {
                                format!("color:{}", hex(c))
                            };
                            format!(
                                "<div class=\"row\"><span style=\"{style}\">● {}{label}</span><span>{} · {ratio:.1}</span></div>",
                                token.name(),
                                hex(c)
                            )
                        } else {
                            let Rgb(or, og, ob) = on;
                            format!(
                                "\x1b[48;2;{br};{bg};{bb}m  \x1b[48;2;{or};{og};{ob}m\x1b[38;2;{r};{g};{b}m● {:<32}\x1b[48;2;{br};{bg};{bb}m\x1b[38;2;{tr};{tg};{tb}m {}  {ratio:>4.1}{:<6}\x1b[0m",
                                format!("{}{label}", token.name()),
                                hex(c),
                                ""
                            )
                        }
                    }
                };
                println!("{line}");
            }
            if html {
                println!("</div>");
            }
        }
        if html {
            println!("</div>");
        }
    }
}
