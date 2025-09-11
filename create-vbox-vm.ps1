# PowerShell script to create a VirtualBox VM for Kosh OS testing

Write-Host "Kosh OS VirtualBox VM Creator" -ForegroundColor Green
Write-Host "=================================" -ForegroundColor Green

# Check if VirtualBox is installed
$vboxManage = $null
$vboxPaths = @(
    "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe",
    "C:\Program Files (x86)\Oracle\VirtualBox\VBoxManage.exe"
)

foreach ($path in $vboxPaths) {
    if (Test-Path $path) {
        $vboxManage = $path
        break
    }
}

if (-not $vboxManage) {
    try {
        $vboxManage = (Get-Command VBoxManage -ErrorAction Stop).Source
    } catch {
        Write-Host "❌ VirtualBox not found!" -ForegroundColor Red
        Write-Host "Please install VirtualBox from: https://www.virtualbox.org/wiki/Downloads" -ForegroundColor Yellow
        Write-Host ""
        Write-Host "After installation, you can manually create a VM using the guide in VIRTUALBOX_SETUP.md" -ForegroundColor Cyan
        exit 1
    }
}

Write-Host "✅ Found VirtualBox at: $vboxManage" -ForegroundColor Green

# Check if ISO exists
if (-not (Test-Path "kosh.iso")) {
    Write-Host "❌ kosh.iso not found!" -ForegroundColor Red
    Write-Host "Please run the build script first:" -ForegroundColor Yellow
    Write-Host "  bash scripts/build-minimal-iso.sh" -ForegroundColor White
    exit 1
}

$isoSize = (Get-Item "kosh.iso").Length / 1MB
Write-Host "✅ Found kosh.iso ($([math]::Round($isoSize, 1)) MB)" -ForegroundColor Green

# VM Configuration
$vmName = "Kosh-OS-Test"
$vmMemory = 256  # MB
$vmVram = 16     # MB

Write-Host ""
Write-Host "Creating VirtualBox VM..." -ForegroundColor Yellow
Write-Host "VM Name: $vmName" -ForegroundColor Cyan
Write-Host "Memory: $vmMemory MB" -ForegroundColor Cyan
Write-Host "Video Memory: $vmVram MB" -ForegroundColor Cyan

try {
    # Check if VM already exists and remove it
    $existingVm = & $vboxManage list vms 2>$null | Select-String $vmName
    if ($existingVm) {
        Write-Host "⚠️  VM '$vmName' already exists. Removing it..." -ForegroundColor Yellow
        & $vboxManage unregistervm $vmName --delete 2>$null
    }
    
    # Create the VM
    Write-Host "Creating VM..." -ForegroundColor Yellow
    & $vboxManage createvm --name $vmName --ostype "Other_64" --register
    if ($LASTEXITCODE -ne 0) { throw "Failed to create VM" }
    
    # Configure VM settings
    Write-Host "Configuring VM settings..." -ForegroundColor Yellow
    & $vboxManage modifyvm $vmName --memory $vmMemory
    & $vboxManage modifyvm $vmName --vram $vmVram
    & $vboxManage modifyvm $vmName --boot1 dvd --boot2 none --boot3 none --boot4 none
    & $vboxManage modifyvm $vmName --acpi on
    & $vboxManage modifyvm $vmName --ioapic on
    & $vboxManage modifyvm $vmName --rtcuseutc on
    & $vboxManage modifyvm $vmName --graphicscontroller vboxvga
    & $vboxManage modifyvm $vmName --audio none
    & $vboxManage modifyvm $vmName --usb off
    & $vboxManage modifyvm $vmName --usbehci off
    & $vboxManage modifyvm $vmName --usbxhci off
    
    # Create and attach storage controller
    Write-Host "Setting up storage..." -ForegroundColor Yellow
    & $vboxManage storagectl $vmName --name "IDE Controller" --add ide --controller PIIX4
    
    # Attach the ISO
    $isoPath = Resolve-Path "kosh.iso"
    Write-Host "Attaching ISO: $isoPath" -ForegroundColor Green
    & $vboxManage storageattach $vmName --storagectl "IDE Controller" --port 0 --device 0 --type dvddrive --medium $isoPath
    
    Write-Host ""
    Write-Host "🎉 VM created successfully!" -ForegroundColor Green
    Write-Host ""
    Write-Host "To start the VM:" -ForegroundColor Cyan
    Write-Host "  VBoxManage startvm `"$vmName`"" -ForegroundColor White
    Write-Host ""
    Write-Host "Or use the VirtualBox GUI:" -ForegroundColor Cyan
    Write-Host "  1. Open VirtualBox" -ForegroundColor White
    Write-Host "  2. Select '$vmName'" -ForegroundColor White
    Write-Host "  3. Click 'Start'" -ForegroundColor White
    
    # Ask if user wants to start the VM
    Write-Host ""
    $startVm = Read-Host "Would you like to start the VM now? (y/n)"
    if ($startVm -eq 'y' -or $startVm -eq 'Y' -or $startVm -eq 'yes') {
        Write-Host "Starting VM..." -ForegroundColor Yellow
        & $vboxManage startvm $vmName
        
        Write-Host ""
        Write-Host "🚀 VM is starting!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Expected boot sequence:" -ForegroundColor Cyan
        Write-Host "  1. GRUB bootloader menu" -ForegroundColor White
        Write-Host "  2. Select 'Kosh Operating System'" -ForegroundColor White
        Write-Host "  3. Platform abstraction layer initialization" -ForegroundColor White
        Write-Host "  4. CPU and memory information display" -ForegroundColor White
        Write-Host "  5. Kernel initialization messages" -ForegroundColor White
    }
    
} catch {
    Write-Host "❌ Error creating VM: $_" -ForegroundColor Red
    Write-Host ""
    Write-Host "Manual setup instructions are available in VIRTUALBOX_SETUP.md" -ForegroundColor Yellow
    exit 1
}

Write-Host ""
Write-Host "📖 For detailed setup instructions, see: VIRTUALBOX_SETUP.md" -ForegroundColor Cyan
Write-Host "🔧 For troubleshooting, check the guide in the same file" -ForegroundColor Cyan