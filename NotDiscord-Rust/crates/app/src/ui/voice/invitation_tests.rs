fn invitation_fixture() -> (NotDiscordUi, egui::Context, tokio::sync::mpsc::UnboundedReceiver<UiAction>, AppSnapshot) {
    let (app,_,commands)=super::super::tests::harness();let ctx=app.ctx.clone();
    let state=AppSnapshot {signed_in:true,voice_connected:true,voice_encryption_ready:true,
        current_user:Some(Person {id:"100".into(),name:"Local".into(),..Default::default()}),
        call_channel_id:"200".into(),call_instance_id:7,call_channel_name:"Synthetic call".into(),
        call_status_label:"Conectado".into(),call_participants:vec![CallParticipant {
            user:Person {id:"100".into(),name:"Local".into(),..Default::default()},is_self:true,..Default::default()}],
        ..Default::default()};
    (app,ctx,commands,state)
}
fn invitation_frame(app:&mut NotDiscordUi,ctx:&egui::Context,state:&AppSnapshot,events:Vec<egui::Event>,budget:f32,width:f32)->egui::FullOutput {
    ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(width,650.))),events,..Default::default()},|ctx| {
        egui::CentralPanel::default().show(ctx,|ui|app.draw_call_in_height(ui,state,budget));
    })
}
fn voice_text_rect(output:&egui::FullOutput,needle:&str)->egui::Rect {
    output.shapes.iter().find_map(|shape|match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text.contains(needle)=>Some(egui::Rect::from_min_size(text.pos,text.galley.size())),_=>None,
    }).unwrap_or_else(||panic!("Missing voice text: {needle}"))
}
fn voice_pointer(position:egui::Pos2,pressed:bool,button:egui::PointerButton)->Vec<egui::Event> {
    vec![egui::Event::PointerMoved(position),egui::Event::PointerButton {pos:position,button,pressed,modifiers:egui::Modifiers::NONE}]
}
#[test]
fn pending_invitees_stay_gray_after_the_ring_stops_without_counting_or_connected_controls() {
    let (mut app,ctx,mut commands,mut state)=invitation_fixture();
    let peer=Person {id:"300".into(),name:"Waiting peer".into(),..Default::default()};
    state.call_pending_invitees=vec![peer.clone()];state.outgoing_call_ringing=false;
    for _ in 0..3 {invitation_frame(&mut app,&ctx,&state,vec![],420.,850.);}
    let output=invitation_frame(&mut app,&ctx,&state,vec![],420.,850.);
    voice_text_rect(&output,"Waiting peer");voice_text_rect(&output,"Llamando...");
    assert_eq!(state.call_participants.len(),1);
    let rect=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("pending-call-tile","account:100:call:200:7","300")))).unwrap();
    assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(shape)
        if shape.rect==rect && shape.fill==Color32::from_rgba_unmultiplied(100,100,100,160))));
    for pressed in [true,false] {invitation_frame(&mut app,&ctx,&state,voice_pointer(rect.center(),pressed,egui::PointerButton::Secondary),420.,850.);}
    let output=invitation_frame(&mut app,&ctx,&state,vec![],420.,850.);
    assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)
        if text.galley.job.text.contains("Volumen del participante"))));
    assert!(commands.try_recv().is_err());
    // A confirmed participant wins even if a coalesced snapshot still contains
    // the prior pending entry; it becomes one ordinary, fully colored tile.
    state.call_participants.push(CallParticipant {user:peer,self_mute:true,self_deaf:true,..Default::default()});
    let joined=invitation_frame(&mut app,&ctx,&state,vec![],420.,850.);
    assert_eq!(joined.shapes.iter().filter(|shape|matches!(&shape.shape,egui::Shape::Text(text)
        if text.galley.job.text=="Waiting peer")).count(),1);
    assert!(!joined.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Llamando...")));
    voice_text_rect(&joined,"Silenciado · Ensordecido");
    state.call_pending_invitees.clear();state.call_participants.truncate(1);
    let ended=invitation_frame(&mut app,&ctx,&state,vec![],420.,850.);
    assert!(!ended.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.job.text=="Waiting peer")));
}
#[test]
fn add_to_call_routes_a_real_invitation_only_for_an_available_conversation_member() {
    let (mut app,ctx,mut commands,mut state)=invitation_fixture();
    state.call_pending_invitees=vec![Person {id:"300".into(),name:"Pending".into(),..Default::default()}];
    state.call_invite_busy_ids=vec!["700".into()];
    state.call_participants.push(CallParticipant {user:Person {id:"500".into(),name:"Connected".into(),..Default::default()},..Default::default()});
    state.call_invite_candidates=vec![
        state.current_user.clone().unwrap(),state.call_pending_invitees[0].clone(),
        Person {id:"400".into(),name:"Previously declined".into(),..Default::default()},
        state.call_participants[1].user.clone(),Person {id:"600".into(),name:"Bot".into(),is_bot:true,..Default::default()},
        Person {id:"700".into(),name:"Invitation being confirmed".into(),..Default::default()},
    ];
    for person in &state.call_invite_candidates {
        let frame=|app:&mut NotDiscordUi,events|ctx.run(egui::RawInput {screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(600.,400.))),events,..Default::default()},|ctx| {
            egui::CentralPanel::default().show(ctx,|ui|app.draw_add_to_call(ui,person,&state));
        });
        frame(&mut app,vec![]);let output=frame(&mut app,vec![]);
        let point=voice_text_rect(&output,"Añadir a la llamada").center();
        for pressed in [true,false] {frame(&mut app,voice_pointer(point,pressed,egui::PointerButton::Primary));}
        if person.id=="400" {
            let action=commands.try_recv().unwrap();
            assert_eq!((action.kind.as_str(),action.id.as_str(),action.channel_id.as_str()),("call-invite","400","200"));
            assert_eq!(serde_json::from_str::<Value>(&action.extra).unwrap()["voiceAttempt"],7);
        }
        assert!(commands.try_recv().is_err(),"self, bots, connected and pending people cannot send duplicate invitations");
    }
}
#[test]
fn local_and_remote_sources_start_as_avatars_and_source_restarts_need_another_explicit_click() {
    for local in [false,true] {
        let (mut app,ctx,mut commands,mut state)=invitation_fixture();
        let owner=if local {"100"}else{"300"};
        state.call_participants=vec![CallParticipant {user:Person {id:owner.into(),name:"Media peer".into(),..Default::default()},
            is_self:local,self_video:true,screen_sharing:true,..Default::default()}];
        let mut textures=Vec::new();
        for (id,kind) in [(11,"camera"),(12,"screen")] {
            let texture=ctx.load_texture(format!("opt-in-{local}-{id}"),egui::ColorImage::filled([320,180],Color32::GREEN),egui::TextureOptions::NEAREST);
            textures.push(texture.id());app.video_textures.insert(id,texture);
            state.call_videos.push(CallVideo {id,user_id:owner.into(),kind:kind.into(),local,active:true,view_requested:false,..Default::default()});
        }
        let has_video=|output:&egui::FullOutput|output.shapes.iter().any(|shape|match &shape.shape {
            egui::Shape::Rect(rect)=>rect.brush.as_ref().is_some_and(|brush|textures.contains(&brush.fill_texture_id)),
            egui::Shape::Mesh(mesh)=>textures.contains(&mesh.texture_id),_=>false,
        });
        for _ in 0..3 {invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);}
        let initial=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
        assert!(!has_video(&initial));voice_text_rect(&initial,"Cámara · Mostrar");voice_text_rect(&initial,"Pantalla · Mostrar");
        assert!(commands.try_recv().is_err(),"publishing a source does not request its playback");
        let camera=voice_text_rect(&initial,"Cámara · Mostrar").center();
        for pressed in [true,false] {invitation_frame(&mut app,&ctx,&state,voice_pointer(camera,pressed,egui::PointerButton::Primary),450.,850.);}
        let selected=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);assert!(has_video(&selected));
        let sent=std::iter::from_fn(||commands.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(sent.len(),1);assert_eq!(sent[0].kind,"view-incoming-media");
        assert_eq!(sent[0].id,owner,"local preview selection addresses only the source sink, never publication");
        state.call_videos[0].active=false;state.call_videos[0].status="error".into();
        state.call_videos[0].error="Synthetic playback failure".into();
        let failed=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
        assert!(!has_video(&failed));voice_text_rect(&failed,"Cámara · Reintentar");voice_text_rect(&failed,"Synthetic playback failure");
        assert!(commands.try_recv().is_err(),"an error in the already chosen source remains reviewable without starting another source");
        state.call_videos[0].active=false;state.call_videos[0].status="paused".into();state.call_participants[0].self_video=false;
        let stopped=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);assert!(!has_video(&stopped));
        let release=commands.try_recv().unwrap();assert_eq!(release.kind,"view-incoming-media");assert_eq!(release.text,"camera");
        assert_eq!(serde_json::from_str::<Value>(&release.extra).unwrap()["visible"],false);
        state.call_videos[0].active=true;state.call_videos[0].status="playing".into();state.call_participants[0].self_video=true;
        let restarted=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);assert!(!has_video(&restarted));
        voice_text_rect(&restarted,"Cámara · Mostrar");
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn requested_watch_survives_its_first_native_id_and_retry_rearms_source_identity() {
    let (mut app,ctx,mut commands,mut state)=invitation_fixture();
    state.call_participants=vec![CallParticipant {user:Person {id:"300".into(),name:"Sharing peer".into(),..Default::default()},
        screen_sharing:true,..Default::default()}];
    state.call_videos=vec![CallVideo {id:u64::MAX-1,user_id:"300".into(),kind:"screen".into(),
        status:"connecting".into(),view_requested:true,..Default::default()}];
    let mut textures=Vec::new();
    for id in [10,11,12] {
        let texture=ctx.load_texture(format!("watch-lifecycle-{id}"),egui::ColorImage::filled([320,180],Color32::GREEN),egui::TextureOptions::NEAREST);
        textures.push(texture.id());app.video_textures.insert(id,texture);
    }
    let has_video=|output:&egui::FullOutput|output.shapes.iter().any(|shape|match &shape.shape {
        egui::Shape::Rect(rect)=>rect.brush.as_ref().is_some_and(|brush|textures.contains(&brush.fill_texture_id)),
        egui::Shape::Mesh(mesh)=>textures.contains(&mesh.texture_id),_=>false,
    });
    for _ in 0..3 {invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);}
    assert!(commands.try_recv().is_err());
    state.call_videos[0].id=10;state.call_videos[0].active=true;state.call_videos[0].status="playing".into();
    let first_native=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
    assert!(has_video(&first_native),"the provisional watch id is replaced by the requested first real source");
    assert!(commands.try_recv().is_err(),"its first native frame must not cancel the explicit watch");

    state.call_videos[0].id=11;
    let restarted=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
    assert!(!has_video(&restarted),"a replacement after real playback still requires an explicit choice");
    let release=commands.try_recv().unwrap();
    assert_eq!((release.kind.as_str(),release.text.as_str()),("view-incoming-media","screen"));
    assert_eq!(serde_json::from_str::<Value>(&release.extra).unwrap()["visible"],false);
    assert!(commands.try_recv().is_err());

    let show=voice_text_rect(&restarted,"Pantalla · Mostrar").center();
    for pressed in [true,false] {invitation_frame(&mut app,&ctx,&state,voice_pointer(show,pressed,egui::PointerButton::Primary),450.,850.);}
    let request=commands.try_recv().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&request.extra).unwrap()["visible"],true);
    assert!(has_video(&invitation_frame(&mut app,&ctx,&state,vec![],450.,850.)));
    state.call_videos[0].active=false;state.call_videos[0].status="error".into();state.call_videos[0].error="Synthetic watch failure".into();
    let failed=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
    let retry=voice_text_rect(&failed,"Pantalla · Reintentar").center();
    voice_text_rect(&failed,"Synthetic watch failure");
    assert!(commands.try_recv().is_err());
    for pressed in [true,false] {invitation_frame(&mut app,&ctx,&state,voice_pointer(retry,pressed,egui::PointerButton::Primary),450.,850.);}
    let retry_actions=std::iter::from_fn(||commands.try_recv().ok()).collect::<Vec<_>>();
    assert_eq!(retry_actions.len(),2);
    assert!(retry_actions.iter().all(|action|action.kind=="view-incoming-media" && action.text=="screen"));
    assert_eq!(serde_json::from_str::<Value>(&retry_actions[0].extra).unwrap()["visible"],false);
    assert_eq!(serde_json::from_str::<Value>(&retry_actions[1].extra).unwrap()["visible"],true);
    state.call_videos[0].id=u64::MAX-2;state.call_videos[0].status="connecting".into();state.call_videos[0].error.clear();
    invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
    assert!(commands.try_recv().is_err());
    state.call_videos[0].id=12;state.call_videos[0].active=true;state.call_videos[0].status="playing".into();
    let retried=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
    assert!(has_video(&retried),"an explicit retry grants a fresh first native source without autoplaying unrelated sources");
    assert!(commands.try_recv().is_err());
}

#[test]
fn selector_previews_only_change_tile_choice_while_published_sources_keep_sink_commands() {
    for (kind,label) in [("camera","Cámara"),("screen","Pantalla")] {
        let (mut app,ctx,mut commands,mut state)=invitation_fixture();
        state.camera_preparing=kind=="camera";state.screen_preparing=kind=="screen";
        state.call_videos=vec![CallVideo {id:31,user_id:"100".into(),kind:kind.into(),local:true,preview:true,
            active:true,status:"playing".into(),..Default::default()}];
        let texture=ctx.load_texture(format!("selector-tile-{kind}"),egui::ColorImage::filled([320,180],Color32::GREEN),egui::TextureOptions::NEAREST);
        let texture_id=texture.id();app.video_textures.insert(31,texture);
        let has_video=|output:&egui::FullOutput|output.shapes.iter().any(|shape|match &shape.shape {
            egui::Shape::Rect(rect)=>rect.brush.as_ref().is_some_and(|brush|brush.fill_texture_id==texture_id),
            egui::Shape::Mesh(mesh)=>mesh.texture_id==texture_id,_=>false,
        });
        let click=|app:&mut NotDiscordUi,state:&AppSnapshot,suffix:&str| {
            let output=invitation_frame(app,&ctx,state,vec![],450.,850.);
            let point=voice_text_rect(&output,&format!("{label} · {suffix}")).center();
            for pressed in [true,false] {invitation_frame(app,&ctx,state,voice_pointer(point,pressed,egui::PointerButton::Primary),450.,850.);}
        };
        for _ in 0..3 {invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);}
        click(&mut app,&state,"Mostrar");
        assert!(has_video(&invitation_frame(&mut app,&ctx,&state,vec![],450.,850.)));
        click(&mut app,&state,"Dejar de ver");
        assert!(!has_video(&invitation_frame(&mut app,&ctx,&state,vec![],450.,850.)));
        assert!(commands.try_recv().is_err(),"preview show/hide stays local to the tile and does not override its selector");

        click(&mut app,&state,"Mostrar");
        invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
        let preview=state.call_videos.pop().unwrap();state.camera_preparing=false;state.screen_preparing=false;
        let removed=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
        assert!(!has_video(&removed));
        assert!(commands.try_recv().is_err(),"resetting a disappeared preview never sends a published-source hide command");
        state.call_videos.push(preview);state.camera_preparing=kind=="camera";state.screen_preparing=kind=="screen";
        click(&mut app,&state,"Mostrar");
        invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
        // Promotion can retain the native id, but publishing is a different
        // lifecycle and needs a new opt-in even after viewing the preview.
        state.call_videos[0].preview=false;state.camera_preparing=false;state.screen_preparing=false;
        state.call_participants[0].self_video=kind=="camera";state.call_participants[0].screen_sharing=kind=="screen";
        let published=invitation_frame(&mut app,&ctx,&state,vec![],450.,850.);
        assert!(!has_video(&published));assert!(commands.try_recv().is_err());
        click(&mut app,&state,"Mostrar");
        assert!(has_video(&invitation_frame(&mut app,&ctx,&state,vec![],450.,850.)));
        let show=commands.try_recv().unwrap();assert_eq!((show.kind.as_str(),show.text.as_str()),("view-incoming-media",kind));
        assert_eq!(serde_json::from_str::<Value>(&show.extra).unwrap()["visible"],true);
        click(&mut app,&state,"Dejar de ver");
        let hide=commands.try_recv().unwrap();assert_eq!((hide.kind.as_str(),hide.text.as_str()),("view-incoming-media",kind));
        assert_eq!(serde_json::from_str::<Value>(&hide.extra).unwrap()["visible"],false);
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn one_call_row_fits_four_people_and_pending_tiles_without_a_gallery_scroll() {
    for width in [730.,520.] {
        let (mut app,ctx,mut commands,mut state)=invitation_fixture();
        state.call_participants[0].self_video=true;state.call_participants[0].screen_sharing=true;
        let peer=Person {id:"300".into(),name:"Joined peer".into(),..Default::default()};
        state.call_participants.push(CallParticipant {user:peer.clone(),self_video:true,screen_sharing:true,
            self_mute:true,self_deaf:true,..Default::default()});
        state.call_pending_invitees=vec![
            Person {id:"400".into(),name:"Pending one".into(),..Default::default()},
            Person {id:"500".into(),name:"Pending two".into(),..Default::default()},
            peer,state.current_user.clone().unwrap(),
        ];
        state.call_invite_candidates=state.call_pending_invitees.clone();
        for _ in 0..4 {invitation_frame(&mut app,&ctx,&state,vec![],350.,width);}
        let output=invitation_frame(&mut app,&ctx,&state,vec![],350.,width);
        let (columns,count,scroll,_,_,offset)=ctx.data(|data|data.get_temp::<(usize,usize,bool,egui::Rect,egui::Vec2,egui::Vec2)>(egui::Id::new("call-gallery-layout"))).unwrap();
        assert_eq!((columns,count,scroll),(4,4,false),"width={width}; pending entries count as tiles, duplicates do not");
        assert_eq!(offset,egui::Vec2::ZERO);
        let scope="account:100:call:200:7";
        let rects=[("participant-avatar","100"),("participant-avatar","300"),("pending-call-tile","400"),("pending-call-tile","500")]
            .map(|(kind,id)|ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new((kind,scope,id)))).unwrap());
        assert!(rects.iter().all(|rect|(rect.top()-rects[0].top()).abs()<0.5),"all four areas share the first row: {rects:?}");
        assert!(rects.iter().all(|rect|rect.width()>=112. && rect.height()>=12.));
        let controls=output.shapes.iter().find_map(|shape|match &shape.shape {
            egui::Shape::Text(text) if matches!(text.galley.job.text.as_str(),"Salir"|"Salir de la llamada") =>
                Some(egui::Rect::from_min_size(text.pos,text.galley.size())), _=>None,
        }).expect("the responsive disconnect control must remain visible");
        for needle in ["Local","Joined peer","Pending one","Pending two","Cámara · Mostrar","Pantalla · Mostrar","Llamando..."] {
            let matching=output.shapes.iter().filter_map(|shape|match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text==needle=>Some((shape.clip_rect,egui::Rect::from_min_size(text.pos,text.galley.size()))),_=>None,
            }).collect::<Vec<_>>();
            assert!(!matching.is_empty(),"missing {needle}, width={width}");
            for (clip,rect) in matching {
                assert!(clip.expand(0.5).contains_rect(rect),"single-row footer clipped: {needle} {rect:?}, clip={clip:?}, width={width}");
                assert!(rect.bottom()<controls.top(),"tile footer overlaps fixed call controls");
            }
        }
        let (viewport,content,offset)=ctx.data(|data|data.get_temp::<(egui::Rect,egui::Vec2,egui::Vec2)>(egui::Id::new("call-panel-overflow-layout"))).unwrap();
        assert!(content.y<=viewport.height()+1.,"a complete row must fit its panel: content={content:?}, viewport={viewport:?}, width={width}");
        assert_eq!(offset,egui::Vec2::ZERO);
        invitation_frame(&mut app,&ctx,&state,vec![egui::Event::PointerMoved(rects[3].center()),
            egui::Event::MouseWheel {unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.,-10000.),modifiers:egui::Modifiers::NONE}],350.,width);
        let after=invitation_frame(&mut app,&ctx,&state,vec![],350.,width);
        assert!((voice_text_rect(&after,"Pending two").top()-voice_text_rect(&output,"Pending two").top()).abs()<0.5);
        assert_eq!(state.call_participants.len(),2,"pending tiles are not connected participants");

        state.call_pending_invitees.push(Person {id:"600".into(),name:"Fifth tile".into(),..Default::default()});
        for _ in 0..3 {invitation_frame(&mut app,&ctx,&state,vec![],350.,width);}
        let (columns,count,scroll,viewport,content,_)=ctx.data(|data|data.get_temp::<(usize,usize,bool,egui::Rect,egui::Vec2,egui::Vec2)>(egui::Id::new("call-gallery-layout"))).unwrap();
        assert_eq!((columns,count,scroll),(4,5,true));
        assert!(content.y>viewport.height(),"a second row has its own internal scroll");
        assert!(commands.try_recv().is_err(),"layout and scrolling do not mutate the call");
    }
}

#[test]
fn call_rows_cap_at_four_and_use_fewer_columns_only_below_the_legible_tile_width() {
    for (width,expected) in [(1600.,4),(370.,2)] {
        let (mut app,ctx,mut commands,mut state)=invitation_fixture();
        state.call_pending_invitees=(0..7).map(|index|Person {id:(300+index).to_string(),name:format!("Pending {index}"),..Default::default()}).collect();
        for _ in 0..3 {invitation_frame(&mut app,&ctx,&state,vec![],350.,width);}
        let (columns,count,scroll,_,_,_)=ctx.data(|data|data.get_temp::<(usize,usize,bool,egui::Rect,egui::Vec2,egui::Vec2)>(egui::Id::new("call-gallery-layout"))).unwrap();
        assert_eq!((columns,count,scroll),(expected,8,true));
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn large_call_gallery_scrolls_its_last_invitation_without_displacing_general_controls() {
    let (mut app,ctx,mut commands,mut state)=invitation_fixture();
    state.call_participants=(0..8).map(|index|CallParticipant {user:Person {id:(1000+index).to_string(),
        name:format!("Connected {index}"),..Default::default()},..Default::default()}).collect();
    state.call_pending_invitees=(0..24).map(|index|Person {id:(2000+index).to_string(),
        name:format!("Pending {index}"),..Default::default()}).collect();
    for _ in 0..4 {invitation_frame(&mut app,&ctx,&state,vec![],460.,850.);}
    let initial=invitation_frame(&mut app,&ctx,&state,vec![],460.,850.);
    let controls=voice_text_rect(&initial,"Salir de la llamada");
    let inside=voice_text_rect(&initial,"Connected 0").center();
    invitation_frame(&mut app,&ctx,&state,vec![egui::Event::PointerMoved(inside),
        egui::Event::MouseWheel {unit:egui::MouseWheelUnit::Point,delta:egui::vec2(0.,-100000.),modifiers:egui::Modifiers::NONE}],460.,850.);
    for _ in 0..8 {invitation_frame(&mut app,&ctx,&state,vec![],460.,850.);}
    let scrolled=invitation_frame(&mut app,&ctx,&state,vec![],460.,850.);
    let last=voice_text_rect(&scrolled,"Pending 23");
    assert!(ctx.content_rect().contains_rect(last));
    let after=voice_text_rect(&scrolled,"Salir de la llamada");
    assert!((after.top()-controls.top()).abs()<0.5,"the participant viewport scrolls without moving the general controls");
    assert!(last.bottom()<after.top());
    assert!(commands.try_recv().is_err(),"scrolling many tiles does not invite, capture or mutate participants");
}
