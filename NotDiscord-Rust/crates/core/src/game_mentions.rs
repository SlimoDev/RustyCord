//! Minimal Discord game catalog projection. No installed-game or profile data.
use crate::{models::MentionGame, parsing::{items,s,valid_id}};
use serde_json::Value;

pub fn search_route(query: &str)->Option<String> {
    let query=query.trim().to_lowercase().replace('_'," ");
    if query.is_empty()||query.chars().count()>100||query.chars().any(char::is_control){return None;}
    let encoded:url::form_urlencoded::Serializer<'_,String>=url::form_urlencoded::Serializer::new(String::new());
    let mut encoded=encoded;encoded.append_pair("q",&query).append_pair("filter_group","default");
    Some(format!("games/autocomplete?{}",encoded.finish()))
}
pub fn resolve_route(ids:&[String])->Option<String> {
    if ids.is_empty()||ids.len()>20||ids.iter().any(|id|!valid_id(id)){return None;}
    let mut encoded=url::form_urlencoded::Serializer::new(String::new());
    // Compatibility inference from the browser Superagent serializer; the
    // supplied artifact leaves its imported serializer unresolved. See ledger.
    for id in ids {encoded.append_pair("game_ids",id);}
    Some(format!("games?{}",encoded.finish()))
}
pub fn parse(value:&Value)->Vec<MentionGame> {
    let mut result=Vec::new();
    for item in items(value).iter().take(100) {
        let id=s(item,"id");let name:String=s(item,"name").chars().filter(|c|!c.is_control()).take(128).collect();
        if !valid_id(&id)||name.trim().is_empty()||excluded(&id)||result.iter().any(|game:&MentionGame|game.id==id){continue;}
        let icon=item.get("icon").or_else(||item.get("media").and_then(|media|media.get("icon")));
        let icon_url=icon.and_then(|icon|match s(icon,"type").as_str(){
            "hash"=>{let hash=s(icon,"value");(hash.len()==32&&hash.bytes().all(|b|b.is_ascii_hexdigit())).then(||format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=64"))},
            "url"=>{let url=s(icon,"value");(url.len()<=2048&&crate::parsing::trusted_media(&url)).then_some(url)},
            _=>None,
        });
        result.push(MentionGame{id,name,icon_url,restricted:restricted(&item["content_classification"])});
        if result.len()==20{break;}
    }
    result
}
fn restricted(value:&Value)->bool {
    let flags=value["discord_classifications"].as_u64().or_else(||value["discord_classifications"].as_str().and_then(|s|s.parse().ok()));
    flags.is_some_and(|flags|flags&25!=0)
        ||value["agency_ratings"]["esrb"]["rating"].as_u64()==Some(5)
        ||value["agency_ratings"]["gop"]["classification"].as_u64()==Some(1)
        ||items(&value["agency_ratings"]["igdb"]["themes"]).iter().any(|v|v.as_u64()==Some(21))
}
fn excluded(id:&str)->bool { EXCLUDED.contains(&id) }
// Public client module 926675; exact source identity recorded in rea-time-game.
const EXCLUDED:&[&str]=&["484425151631589378","1124351896166076496","1124351975702679583","1124353289857810612","1124353460880556183","1124353599590379552","1124354036020297778","1124354174310699169","1124354219374297238","1124354724301393940","1124355292591829002","1124355334593593364","1124355447323906128","1124355979383943238","1124356108509786252","1124356199043846164","1124357045823488140","1124357133526384761","1124357256008450188","1124357277042888804","1124357563014725652","1124357621219086427","1124357688621551638","1124358061314818138","1124358311232426086","1124358369315139704","1124358585397293186","1124358634739093554","1124358645010935888","1124358853446873148","1124359047932555324","1124359276488573050","1124359293873963008","1124359312932884551","1124359434689335407","1124359758556708915","1124360177634791564","1124360363962552450","1124360393519804557","1124360545580093551","1124360662131425361","1124360678858309652","1129504691517136927","1129504937424986152","1129505486010589265","1130001171172561006","1131042996108927056","1132690430694264872","1138683327470108773","1185733153193725952","1201723489271431208","1202901407431266324","1216022702519746742","1222062501890625598","1251038428578189345","1262015762298830888","1264748775877120030","1294962893057429605","1297635514555306004","1328800843922804786","1340125468736618546","1345440503465906216","1366277825207861270","1370943538572824626","1373812493151113266","1381135699486445642","1384291534962430072","1428188279433597048","1429242833394729142","1441988763529117706","1445221962757701702","1445976787174686730","1447467809293733930","1447664345839571158","1455772568979116103","1458671686131122269","1459562537145077832"];
