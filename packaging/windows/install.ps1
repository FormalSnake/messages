# Installs a release build to %LOCALAPPDATA%\Programs\Messages with a Start menu shortcut carrying
# the app id nd sets on the process, so the taskbar groups the window under the shortcut icon.
param([string]$Exe = "$PSScriptRoot\..\..\target\release\messages.exe")
$ErrorActionPreference = 'Stop'
$dir = "$env:LOCALAPPDATA\Programs\Messages"
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $Exe "$dir\Messages.exe" -Force
Copy-Item "$PSScriptRoot\messages.ico" "$dir\Messages.ico" -Force

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Runtime.InteropServices.ComTypes;
[ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IShellLinkW {
  void GetPath(IntPtr a, int b, IntPtr c, int d); void GetIDList(out IntPtr p); void SetIDList(IntPtr p);
  void GetDescription(IntPtr a, int b); void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string s);
  void GetWorkingDirectory(IntPtr a, int b); void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string s);
  void GetArguments(IntPtr a, int b); void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string s);
  void GetHotkey(out short h); void SetHotkey(short h); void GetShowCmd(out int c); void SetShowCmd(int c);
  void GetIconLocation(IntPtr a, int b, out int c); void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string s, int i);
  void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string s, int r); void Resolve(IntPtr h, int f);
  void SetPath([MarshalAs(UnmanagedType.LPWStr)] string s);
}
[ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IPropertyStore {
  void GetCount(out uint c); void GetAt(uint i, out PropertyKey k); void GetValue(ref PropertyKey k, out PropVariant v);
  void SetValue(ref PropertyKey k, ref PropVariant v); void Commit();
}
[StructLayout(LayoutKind.Sequential, Pack = 4)] public struct PropertyKey { public Guid fmtid; public uint pid; }
[StructLayout(LayoutKind.Explicit)] public struct PropVariant { [FieldOffset(0)] public ushort vt; [FieldOffset(8)] public IntPtr p; }
[ComImport, Guid("00021401-0000-0000-C000-000000000046")] public class ShellLink {}
public static class Lnk {
  public static void Create(string lnk, string target, string icon, string appId) {
    var link = (IShellLinkW)new ShellLink();
    link.SetPath(target); link.SetIconLocation(icon, 0); link.SetDescription("Messages");
    link.SetWorkingDirectory(System.IO.Path.GetDirectoryName(target));
    var store = (IPropertyStore)link;
    var key = new PropertyKey { fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), pid = 5 };
    var value = new PropVariant { vt = 31, p = Marshal.StringToCoTaskMemUni(appId) };
    store.SetValue(ref key, ref value); store.Commit();
    ((IPersistFile)link).Save(lnk, true);
    Marshal.FreeCoTaskMem(value.p);
  }
}
"@
$programs = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs"
[Lnk]::Create("$programs\Messages.lnk", "$dir\Messages.exe", "$dir\Messages.ico", "dev.formalsnake.messages")
$key = 'HKCU:\Software\Classes\AppUserModelId\dev.formalsnake.messages'
New-Item $key -Force | Out-Null
Set-ItemProperty $key DisplayName 'Messages'
Set-ItemProperty $key IconUri "$dir\Messages.ico"
Get-Item "$programs\Messages.lnk" | Select-Object Name, Length
"installed $dir\Messages.exe"
