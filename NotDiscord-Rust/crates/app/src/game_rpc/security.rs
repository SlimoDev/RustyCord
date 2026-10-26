//! Same-user, same-session named pipes. No process-memory access or persisted paths.
use std::{ffi::c_void, io, os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle}};
use tokio::net::windows::named_pipe::{NamedPipeServer,ServerOptions};

pub(super) struct Context { sid:String, session:u32 }
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Peer { pub pid:u32, pub executable:String }

impl Context {
    pub(super) fn current()->Result<Self,String> {
        let sid=unsafe {process_sid(GetCurrentProcess())}?;
        let mut session=0;
        if unsafe {ProcessIdToSessionId(std::process::id(),&mut session)}==0 {
            return Err("No se pudo comprobar la sesión de Rich Presence.".into());
        }
        Ok(Self {sid,session})
    }
    pub(super) fn pipe(&self,name:&str,first:bool)->io::Result<NamedPipeServer> {
        let sddl=format!("D:P(A;;GA;;;{})",self.sid).encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut descriptor=std::ptr::null_mut();
        if unsafe {ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut descriptor,std::ptr::null_mut())}==0 {
            return Err(io::Error::last_os_error());
        }
        let mut security=SecurityAttributes {length:std::mem::size_of::<SecurityAttributes>() as u32,descriptor,inherit:0};
        let result=unsafe {
            ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).max_instances(9)
                .create_with_security_attributes_raw(name,(&mut security as *mut SecurityAttributes).cast())
        };
        unsafe {LocalFree(descriptor);}
        result
    }
    pub(super) fn verify(&self,pipe:&NamedPipeServer)->Result<Peer,String> {
        let mut pid=0;
        if unsafe {GetNamedPipeClientProcessId(pipe.as_raw_handle(),&mut pid)}==0 || pid==0 {
            return Err("No se pudo verificar el proceso de Rich Presence.".into());
        }
        let mut session=u32::MAX;
        if unsafe {ProcessIdToSessionId(pid,&mut session)}==0 || session!=self.session {
            return Err("El cliente de Rich Presence pertenece a otra sesión.".into());
        }
        let handle=unsafe {OpenProcess(0x1000,0,pid)}; // QUERY_LIMITED_INFORMATION only.
        if handle.is_null() {return Err("No se pudo verificar el cliente de Rich Presence.".into());}
        let handle=unsafe {OwnedHandle::from_raw_handle(handle)};
        if unsafe {process_sid(handle.as_raw_handle())}?!=self.sid {
            return Err("El cliente de Rich Presence pertenece a otro usuario.".into());
        }
        let mut path=[0u16;4096];let mut length=path.len() as u32;
        if unsafe {QueryFullProcessImageNameW(handle.as_raw_handle(),0,path.as_mut_ptr(),&mut length)}==0
            ||length==0||length as usize>path.len() {
            return Err("No se pudo identificar el ejecutable de Rich Presence.".into());
        }
        let path=String::from_utf16(&path[..length as usize]).map_err(|_|"Ejecutable de Rich Presence inválido.")?;
        let executable=path.rsplit(['\\','/']).next().unwrap_or("").to_owned();
        if executable.is_empty()||executable.len()>1024||executable.chars().any(char::is_control) {
            return Err("Ejecutable de Rich Presence inválido.".into());
        }
        Ok(Peer {pid,executable})
    }
}

unsafe fn process_sid(process:*mut c_void)->Result<String,String> {
    unsafe {
        let mut token=std::ptr::null_mut();
        if OpenProcessToken(process,8,&mut token)==0 {return Err("No se pudo verificar el usuario de Rich Presence.".into());}
        let token=OwnedHandle::from_raw_handle(token);let mut needed=0;
        GetTokenInformation(token.as_raw_handle(),1,std::ptr::null_mut(),0,&mut needed);
        if needed<std::mem::size_of::<TokenUser>() as u32||needed>65536 {return Err("Identidad de Rich Presence inválida.".into());}
        let mut buffer=vec![0u8;needed as usize];
        if GetTokenInformation(token.as_raw_handle(),1,buffer.as_mut_ptr().cast(),needed,&mut needed)==0 {
            return Err("No se pudo leer la identidad de Rich Presence.".into());
        }
        let user=std::ptr::read_unaligned(buffer.as_ptr().cast::<TokenUser>());
        let mut value=std::ptr::null_mut();
        if ConvertSidToStringSidW(user.sid,&mut value)==0 {return Err("Identidad de Rich Presence inválida.".into());}
        let mut length=0;while length<256&&*value.add(length)!=0 {length+=1;}
        let result=if length==256 {Err("Identidad de Rich Presence inválida.".into())}
            else {String::from_utf16(std::slice::from_raw_parts(value,length)).map_err(|_|"Identidad de Rich Presence inválida.".into())};
        LocalFree(value.cast());result
    }
}
#[repr(C)]
struct SecurityAttributes {length:u32,descriptor:*mut c_void,inherit:i32}
#[repr(C)]
struct TokenUser {sid:*mut c_void,attributes:u32}
#[link(name="advapi32")]
unsafe extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(s:*const u16,version:u32,descriptor:*mut *mut c_void,size:*mut u32)->i32;
    fn OpenProcessToken(process:*mut c_void,access:u32,token:*mut *mut c_void)->i32;
    fn GetTokenInformation(token:*mut c_void,class:u32,information:*mut c_void,size:u32,needed:*mut u32)->i32;
    fn ConvertSidToStringSidW(sid:*mut c_void,string:*mut *mut u16)->i32;
}
#[link(name="kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess()->*mut c_void;
    fn OpenProcess(access:u32,inherit:i32,pid:u32)->*mut c_void;
    fn LocalFree(pointer:*mut c_void)->*mut c_void;
    fn GetNamedPipeClientProcessId(pipe:*mut c_void,pid:*mut u32)->i32;
    fn ProcessIdToSessionId(pid:u32,session:*mut u32)->i32;
    fn QueryFullProcessImageNameW(process:*mut c_void,flags:u32,path:*mut u16,size:*mut u32)->i32;
}
