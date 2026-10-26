use super::*;

fn tokens() -> &'static regex::Regex {
    static TOKENS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    TOKENS.get_or_init(|| {
        regex::Regex::new(r"<(a?):([A-Za-z0-9_]{2,64}):([0-9]{1,22})>|:([A-Za-z0-9_]{2,64}):|<@!?([0-9]{1,22})>")
            .unwrap()
    })
}
fn parsed_emoji(capture: &regex::Captures<'_>, state: &AppSnapshot) -> Option<StatusEmoji> {
    if let Some(id) = capture.get(3) {
        if !notdiscord_core::parsing::valid_id(id.as_str()) {
            return None;
        }
        return Some(StatusEmoji {
            id: id.as_str().into(),
            name: capture[2].into(),
            animated: &capture[1] == "a",
        });
    }
    let name = capture.get(4)?.as_str();
    let mut found = state
        .emojis
        .iter()
        .filter(|emoji| emoji.name == name && notdiscord_core::parsing::valid_id(&emoji.id));
    let first = found.next()?;
    if found.any(|emoji| emoji.id != first.id) {
        return None;
    }
    Some(StatusEmoji {
        id: first.id.clone(),
        name: first.name.clone(),
        animated: first.animated,
    })
}
pub(super) fn emoji_token(text:&str,state:&AppSnapshot)->Option<StatusEmoji> {
    let capture=tokens().captures(text)?;
    (capture.get(0)?.as_str()==text).then(||parsed_emoji(&capture,state)).flatten()
}
impl NotDiscordUi {
    /// Inline custom emoji rendering for text surfaces outside message bodies.
    /// Keeps unknown/ambiguous tokens readable and never opens media or links.
    pub(super) fn rich_custom_text(
        &mut self,
        ui: &mut egui::Ui,
        text: &str,
        state: &AppSnapshot,
        size: f32,
    ) {
        if !self.pictures.has_emoji_scope() {
            self.emoji_section(ui,("rich-custom-text",text),|this,ui| this.rich_custom_text_inner(ui,text,state,size));
        } else {
            self.rich_custom_text_inner(ui,text,state,size);
        }
    }
    fn rich_custom_text_inner(&mut self,ui:&mut egui::Ui,text:&str,state:&AppSnapshot,size:f32) {
        let mut line_start = 0;
        for line in text.split('\n') {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.;
                ui.spacing_mut().item_spacing.y = 0.;
                let mut end = 0;
                for capture in tokens().captures_iter(line) {
                    let whole = capture.get(0).unwrap();
                    let user = capture.get(5).filter(|id| {
                        notdiscord_core::parsing::valid_id(id.as_str())
                            && mentions::plain_text_at(text, line_start + whole.start())
                    });
                    let emoji = parsed_emoji(&capture, state);
                    if user.is_none() && emoji.is_none() { continue; }
                    if whole.start() > end {
                        ui.add(
                            egui::Label::new(RichText::new(&line[end..whole.start()]).size(size))
                                .wrap(),
                        );
                    }
                    if let Some(id) = user {
                        if mentions::label(ui, mentions::name(id.as_str(), &[], state), size).clicked() {
                            self.send("profile", id.as_str(), "", "", &state.selected_channel_id);
                        }
                    } else if let Some(emoji) = emoji {
                        self.emoji(ui, &emoji, size * 1.4);
                    }
                    end = whole.end();
                }
                if end < line.len() {
                    ui.add(egui::Label::new(RichText::new(&line[end..]).size(size)).wrap());
                }
                if line.is_empty() {
                    ui.add_space(size);
                }
            });
            line_start += line.len() + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn has_texture(frame:&egui::FullOutput,id:egui::TextureId)->bool {
        frame.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Mesh(mesh) if mesh.texture_id==id))
    }
    fn seed_emojis(app:&mut NotDiscordUi, animated:bool)->Vec<egui::TextureId> {
        let mut ids=Vec::new();
        for id in ["401","402"] {
            let url=StatusEmoji {id:id.into(),name:"Synthetic".into(),animated:true}.image_url().unwrap();
            if animated {ids.extend(app.pictures.seed_emoji_animation_fixture(&app.ctx,&url,2));}
            else {ids.push(app.pictures.seed_sized_fixture(&app.ctx,&url.replace("&animated=true",""),64));}
        }
        ids
    }
    fn section_frame(app:&mut NotDiscordUi,pointer:Option<egui::Pos2>,minimized:bool,hidden:bool,clipped:bool)->egui::FullOutput {
        let ctx=app.ctx.clone();
        let mut input=egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(700.,500.))),..Default::default()};
        input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().minimized=Some(minimized);
        input.events.push(pointer.map_or(egui::Event::PointerGone,egui::Event::PointerMoved));
        app.pictures.begin_frame();
        let output=ctx.run(input,|ctx|egui::CentralPanel::default().show(ctx,|ui| {
            if clipped {ui.set_clip_rect(egui::Rect::from_min_size(egui::pos2(0.,0.),egui::vec2(700.,20.)));ui.add_space(80.);}
            if !hidden {
                app.emoji_section(ui,"synthetic-profile-section",|this,ui| {
                    ui.label("Hover the entire profile section");
                    this.rich_custom_text(ui,"<a:First:401> text <a:Second:402>",&AppSnapshot::welcome(),18.);
                });
            }
        }).inner);
        app.pictures.finish_frame(minimized);
        output
    }
    #[test]
    fn whole_section_hover_replaces_all_static_emojis_on_the_first_hover_frame_and_restores_them_on_exit() {
        let (mut app,_,_)=super::super::tests::harness();
        app.prefs.allow_gif_playback=true;
        let posters=seed_emojis(&mut app,false);
        let first=section_frame(&mut app,None,false,false,false);
        for &id in &posters {assert!(has_texture(&first,id));}
        let pointer=first.shapes.iter().find_map(|shape|match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text=="Hover the entire profile section"=>Some(text.pos+egui::vec2(5.,5.)),_=>None,
        }).unwrap();
        let animations=seed_emojis(&mut app,true);
        let hovered=section_frame(&mut app,Some(pointer),false,false,false);
        assert_eq!(app.pictures.custom_animation_metrics()["requested"],2);
        for id in [animations[0],animations[2]] {assert!(has_texture(&hovered,id),"Hovering ordinary section text must activate every nested emoji immediately");}
        for &id in &posters {assert!(!has_texture(&hovered,id),"Replace the poster rather than layering a transparent animation above it");}
        let exited=section_frame(&mut app,Some(egui::pos2(650.,450.)),false,false,false);
        for &id in &posters {assert!(has_texture(&exited,id));}
        assert_eq!(app.pictures.custom_animation_metrics()["entries"],0);
        assert_eq!(app.pictures.custom_animation_metrics()["pending"],0);
        assert_eq!(app.pictures.custom_animation_metrics()["worker"],false);
        assert_eq!(app.pictures.pending_fixture_requests(),0);
    }
    #[test]
    fn hidden_clipped_minimized_and_disabled_sections_do_not_keep_custom_animations() {
        let (mut app,_,_)=super::super::tests::harness();
        app.prefs.allow_gif_playback=true;
        seed_emojis(&mut app,false);
        for (minimized,hidden,clipped,allowed) in [(false,true,false,true),(false,false,true,true),(true,false,false,true),(false,false,false,false)] {
            let animations=seed_emojis(&mut app,true);
            app.prefs.allow_gif_playback=allowed;
            let output=section_frame(&mut app,Some(egui::pos2(20.,15.)),minimized,hidden,clipped);
            assert_eq!(app.pictures.custom_animation_metrics()["entries"],0,"minimized={minimized}, hidden={hidden}, clipped={clipped}, allowed={allowed}");
            assert_eq!(app.pictures.custom_animation_metrics()["requested"],0);
            assert_eq!(app.pictures.custom_animation_metrics()["worker"],false);
            assert!(!animations.iter().any(|&id|has_texture(&output,id)));
        }
        assert_eq!(app.pictures.pending_fixture_requests(),0);
    }
    #[test]
    fn an_unscoped_custom_emoji_is_static_even_when_the_pointer_is_over_its_image() {
        let (mut app,_,_)=super::super::tests::harness();
        app.prefs.allow_gif_playback=true;
        let posters=seed_emojis(&mut app,false);
        let animations=seed_emojis(&mut app,true);
        let ctx=app.ctx.clone();
        app.pictures.begin_frame();
        let frame=ctx.run(egui::RawInput {events:vec![egui::Event::PointerMoved(egui::pos2(15.,15.))],..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui| {
                app.emoji(ui,&StatusEmoji {id:"401".into(),name:"Synthetic".into(),animated:true},26.);
            });
        });
        app.pictures.finish_frame(false);
        assert!(has_texture(&frame,posters[0]));
        assert!(!animations.iter().any(|&id|has_texture(&frame,id)));
        assert_eq!(app.pictures.custom_animation_metrics()["requested"],0);
        assert_eq!(app.pictures.custom_animation_metrics()["entries"],0);
        assert_eq!(app.pictures.custom_animation_metrics()["worker"],false);
    }
    #[test]
    fn bio_explicit_custom_tokens_keep_ids_and_animation_without_a_guild_catalog() {
        let state = AppSnapshot::welcome();
        let bio = "<a:MoniChibiSpin:459361793035206666><:Other:459361793056309250>";
        let emojis: Vec<_> = tokens()
            .captures_iter(bio)
            .filter_map(|capture| parsed_emoji(&capture, &state))
            .collect();
        assert_eq!(emojis.len(), 2);
        assert!(emojis[0].animated);
        assert_eq!(emojis[0].id, "459361793035206666");
        assert!(!emojis[1].animated);
        assert!(
            tokens()
                .captures_iter(":unknown:")
                .all(|capture| parsed_emoji(&capture, &state).is_none())
        );
    }
}
