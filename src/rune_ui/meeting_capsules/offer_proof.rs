// Fixtures supply only the inputs to detection. They do not supply titles,
// layout, buttons, springs or an alternate view. No microphone is opened.
fn provider_cases() -> Vec<(&'static str,MeetingCandidate)> {
    use crate::dictation::context::ContextSnapshot;
    use crate::meeting::detection::candidate_from_microphone;
    use crate::microphone_activity::ActiveMicrophoneApplication;
    const BRAVE:&str="com.brave.Browser";
    [ ("meet",BRAVE,"Brave Browser",Some("https://meet.google.com/abc-defg-hij"),"Google Meet"),
      ("zoom","us.zoom.xos","zoom.us",None,""),
      ("teams","com.microsoft.teams2","Microsoft Teams",None,""),
      ("facetime","com.apple.FaceTime","FaceTime",None,""),
      ("banshee",BRAVE,"Brave Browser",Some("https://banshee.app/demo"),""),
      ("slack","com.tinyspeck.slackmacgap","Slack",None,""),
      ("unknown",BRAVE,"Brave Browser",None,""),
      ("named",BRAVE,"Brave Browser",Some("https://meet.google.com/abc-defg-hij"),"Design review - Google Meet")]
      .into_iter().map(|(slug,bundle,name,url,title)|{
         let app=ActiveMicrophoneApplication{pid:42,bundle_id:bundle.into(),name:name.into()};
         let cx=ContextSnapshot{application:Some(name.into()),window_title:Some(title.into()),browser_url:url.map(|s|s.parse().unwrap()),..Default::default()};
         (slug,candidate_from_microphone(&app,&cx).unwrap())
      }).collect()
}

#[test]
fn detected_offer_sheet(){
    use crate::rune_ui::offscreen;
    use std::fmt::Write;
    let Some(dir)=std::env::var_os("HEX_DETECTED_OFFERS") else{return};let dir=std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let gpu=rune::render::Gpu::try_new(rune::render::Gpu::instance(),None).unwrap();let mut renderer=offscreen::renderer(&gpu.device);
    let mut report=String::new();
    for lang in ["en","fr"] {crate::desktop::i18n::apply(Some(lang));
    for (slug,candidate) in provider_cases(){for dark in [true,false]{
        let (mut rt,commands,chosen)=runtime(offer(candidate.clone()),false,dark,PANEL);
        let size=rt.state.presentation.window();rt.transparent=true;rt.set_viewport(size,2.0);
        let native=offer_surface::local_bundle(&candidate).and_then(offer_surface::native_icon);
        if lang=="en" && dark {writeln!(report,"{slug}\t{}\t{}\t{}",candidate.offer_title(),if native.is_some(){"installed app"}else{offer_surface::fallback_name(&candidate)},size.0).unwrap();}
        rt.update(move |c,tx|{c.presentation.icon=native;tx.instant()});rt.frame(0.);rt.settle_all();rt.frame(0.);
        let w=(size.0*2.0) as u32;let h=(size.1*2.0) as u32;
        let target=offscreen::target(&gpu.device,w,h);
        let count=if slug=="slack" {96}else{1};
        let theme=if dark{"dark"}else{"light"};
        for frame in 0..count {
            if frame==18 {tap(&mut rt,"offer-more");}else if frame==60 {tap(&mut rt,"offer-cancel");}
            advance_offer(&mut rt,frame as f64/60.0);
            let bytes=offscreen::paint(&gpu,&mut renderer,&target,&mut rt);
            rune::render::write_png(&dir.join(format!("{lang}-{theme}-{slug}-{frame:03}.png")),w,h,&bytes).unwrap();
        }
        assert!(commands.try_recv().is_err() && chosen.lock().unwrap().is_empty());
    }}}
    std::fs::write(dir.join("providers.tsv"),report).unwrap();
}
