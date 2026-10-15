on run
  set control to quoted form of POSIX path of (path to resource "control.sh")
  set binaryPath to paragraph 1 of (read (path to resource "binary.txt") as «class utf8»)
  set commandPrefix to control & " "
  set binaryArgument to " " & quoted form of binaryPath
  try
    set statusText to do shell script commandPrefix & "status" & binaryArgument
    activate
    set choice to button returned of (display dialog statusText with title "macmon Web Control" buttons {"取消", "停止", "启动并打开"} default button "启动并打开" cancel button "取消")
    if choice is "停止" then
      do shell script commandPrefix & "stop" & binaryArgument
      display dialog "后台网页监测已停止。" with title "macmon Web Control" buttons {"好"} default button "好"
    else
      do shell script commandPrefix & "open" & binaryArgument
    end if
  on error messageText number errorNumber
    if errorNumber is not -128 then display alert "macmon Web Control" message messageText as critical
  end try
end run
