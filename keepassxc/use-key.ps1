# 将 KeePass 中的 API Key 注入当前 PowerShell 会话环境变量
# 用法:
#   . .\use-key.ps1 openai-server-prod
#   . .\use-key.ps1 openai-server-prod -VarName OPENAI_API_KEY
#
# 注意: 必须用「点空格」source 方式执行，子进程里设的 env 才会留在当前会话

param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Title,

    [string]$VarName = ""
)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $here

$argsList = @("key_helper.py", "export-env", $Title, "--shell", "powershell", "--print-only")
if ($VarName) {
    $argsList += @("--var", $VarName)
}

$line = & python @argsList
if ($LASTEXITCODE -ne 0) {
    throw "key_helper 失败 (exit $LASTEXITCODE)"
}

Invoke-Expression $line
Write-Host "已注入环境变量。用 `$env:变量名 验证；关闭终端后失效。"
