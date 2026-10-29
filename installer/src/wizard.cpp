#include "wizard.h"
#include "installation.h"

#include <QCheckBox>
#include <QDir>
#include <QFileDialog>
#include <QFileInfo>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QProcess>
#include <QProgressBar>
#include <QPushButton>
#include <QStorageInfo>
#include <QVBoxLayout>

static QString humanBytes(qint64 n)
{
    return QLocale().formattedDataSize(n, 1, QLocale::DataSizeTraditionalFormat);
}

InstallerWizard::InstallerWizard(QWidget *parent) : QWizard(parent)
{
    setWindowTitle(QStringLiteral("Gauntlet: Dark Legacy — Installer"));
    setWizardStyle(QWizard::ModernStyle);
    setMinimumSize(720, 520);
    setOption(QWizard::NoBackButtonOnLastPage, true);
    setOption(QWizard::NoCancelButtonOnLastPage, true);

    auto *welcome = new QWizardPage;
    welcome->setTitle(QStringLiteral("Welcome"));
    welcome->setSubTitle(QStringLiteral("Install the native PC version of Gauntlet: Dark Legacy."));
    auto *wl = new QVBoxLayout(welcome);
    auto *text = new QLabel(QStringLiteral(
        "<p>This installs the native port of <b>Gauntlet: Dark Legacy</b> (PlayStation 2) for this computer.</p>"
        "<p>The port contains none of the game's own data. You need <b>your own copy of the PlayStation 2 game</b> "
        "(USA, SLUS-20047): the disc in an optical drive, or an image of it (.iso or .bin). "
        "The installer reads the game's files from it and checks each one against the supported release.</p>"
        "<p>About 1 GB of disk space is needed. No PlayStation 2 BIOS or emulator is used.</p>"
        "<p>The port is free software under the GNU General Public License, version 3 or later "
        "(LICENSE and THIRD_PARTY.md are installed beside the game).</p>"));
    text->setWordWrap(true);
    text->setTextFormat(Qt::RichText);
    wl->addWidget(text);
    wl->addStretch();
    setPage(WelcomePage, welcome);
    setPage(DiscPage, new DiscSelectPage(this));
    setPage(DestinationPage, new DestinationSelectPage(this));
    setPage(InstallPage, new InstallProgressPage(this));
    setPage(DonePage, new FinishedPage(this));
}

void InstallerWizard::reject()
{
    auto *p = qobject_cast<InstallProgressPage *>(page(InstallPage));
    if (p && p->busy()) {
        if (QMessageBox::question(this, windowTitle(), QStringLiteral("Stop the installation? Nothing is kept of an unfinished import."))
            != QMessageBox::Yes)
            return;
        p->cancelInstall();
        return;
    }
    QWizard::reject();
}

DiscSelectPage::DiscSelectPage(InstallerWizard *w) : m_wizard(w)
{
    setTitle(QStringLiteral("Your game disc"));
    setSubTitle(QStringLiteral("Choose the drive with the Gauntlet: Dark Legacy disc, or an image of the disc."));
    auto *l = new QVBoxLayout(this);
    m_list = new QListWidget;
    l->addWidget(new QLabel(QStringLiteral("Drives with a disc:")));
    l->addWidget(m_list);
    auto *buttons = new QHBoxLayout;
    auto *rescan = new QPushButton(QStringLiteral("Look again"));
    auto *image = new QPushButton(QStringLiteral("Disc image file…"));
    auto *folder = new QPushButton(QStringLiteral("Folder…"));
    folder->setToolTip(QStringLiteral("A folder holding the disc's files (for example a disc mounted elsewhere)."));
    buttons->addWidget(rescan);
    buttons->addStretch();
    buttons->addWidget(image);
    buttons->addWidget(folder);
    l->addLayout(buttons);
    m_status = new QLabel;
    m_status->setWordWrap(true);
    m_status->setMinimumHeight(48);
    l->addWidget(m_status);
    m_probe = new Probe(this);
    connect(rescan, &QPushButton::clicked, this, &DiscSelectPage::rescan);
    connect(image, &QPushButton::clicked, this, [this]() {
        const QString f = QFileDialog::getOpenFileName(this, QStringLiteral("Disc image"), QDir::homePath(),
                                                       QStringLiteral("Disc images (*.iso *.ISO *.bin *.BIN *.img *.IMG);;All files (*)"));
        if (!f.isEmpty())
            choose(f, QFileInfo(f).fileName());
    });
    connect(folder, &QPushButton::clicked, this, [this]() {
        const QString d = QFileDialog::getExistingDirectory(this, QStringLiteral("Folder with the disc's files"), QDir::homePath());
        if (!d.isEmpty())
            choose(d, QDir::toNativeSeparators(d));
    });
    connect(m_list, &QListWidget::currentRowChanged, this, [this](int row) {
        if (row >= 0)
            choose(m_list->item(row)->data(Qt::UserRole).toString(), m_list->item(row)->text());
    });
    connect(m_probe, &Probe::finished, this, [this](const QString &source, bool ok, const QString &message) {
        if (source != m_chosen)
            return;
        m_supported = ok;
        m_status->setText((ok ? QStringLiteral("✔ ") : QStringLiteral("✘ ")) + message.toHtmlEscaped());
        if (ok)
            m_wizard->source = source;
        emit completeChanged();
    });
}

void DiscSelectPage::initializePage()
{
    rescan();
}

void DiscSelectPage::rescan()
{
    m_list->clear();
    const auto found = discCandidates();
    for (const DiscCandidate &c : found) {
        auto *item = new QListWidgetItem(c.label, m_list);
        item->setData(Qt::UserRole, c.path);
    }
    if (found.isEmpty())
        m_status->setText(QStringLiteral("No disc found in a drive. Insert the disc and choose “Look again”, or choose a disc image file."));
    else
        m_list->setCurrentRow(0);
}

void DiscSelectPage::choose(const QString &path, const QString &label)
{
    m_chosen = path;
    m_supported = false;
    m_status->setText(QStringLiteral("Checking %1 …").arg(label.toHtmlEscaped()));
    emit completeChanged();
    m_probe->start(path);
}

bool DiscSelectPage::isComplete() const
{
    return m_supported && !m_probe->running();
}

DestinationSelectPage::DestinationSelectPage(InstallerWizard *w) : m_wizard(w)
{
    setTitle(QStringLiteral("Where to install"));
    // Leaving this page starts the installation: its Next is "Install" and
    // there is no way back into a running import.
    setCommitPage(true);
    setButtonText(QWizard::CommitButton, QStringLiteral("Install"));
    setSubTitle(QStringLiteral("The game is installed into a folder of its own. Your saved characters are kept separately and survive a reinstall."));
    auto *l = new QVBoxLayout(this);
    auto *row = new QHBoxLayout;
    m_path = new QLineEdit;
    auto *browse = new QPushButton(QStringLiteral("Browse…"));
    row->addWidget(m_path);
    row->addWidget(browse);
    l->addWidget(new QLabel(QStringLiteral("Install folder:")));
    l->addLayout(row);
    m_space = new QLabel;
    m_space->setWordWrap(true);
    l->addWidget(m_space);
    m_menu = new QCheckBox(
#ifdef Q_OS_WIN
        QStringLiteral("Add to the Start menu")
#else
        QStringLiteral("Add to the applications menu")
#endif
    );
    m_desktop = new QCheckBox(QStringLiteral("Create a desktop shortcut"));
    m_menu->setChecked(true);
    m_desktop->setChecked(true);
    l->addSpacing(12);
    l->addWidget(m_menu);
    l->addWidget(m_desktop);
    l->addStretch();
    connect(m_path, &QLineEdit::textChanged, this, &DestinationSelectPage::update);
    connect(browse, &QPushButton::clicked, this, [this]() {
        const QString d = QFileDialog::getExistingDirectory(this, QStringLiteral("Install folder"), QDir::homePath());
        if (!d.isEmpty())
            m_path->setText(QDir::toNativeSeparators(QDir(d).filePath(QStringLiteral("GauntletDarkLegacy"))));
    });
}

void DestinationSelectPage::initializePage()
{
    if (m_path->text().isEmpty())
        m_path->setText(QDir::toNativeSeparators(QDir(QDir::homePath()).filePath(QStringLiteral("GauntletDarkLegacy"))));
    update();
}

void DestinationSelectPage::update()
{
    const QString dest = QDir::fromNativeSeparators(m_path->text().trimmed());
    m_ok = false;
    if (dest.isEmpty() || QFileInfo(dest).isRelative()) {
        m_space->setText(QStringLiteral("Choose a full folder path."));
    } else {
        // The nearest existing parent decides the volume.
        QString probe = dest;
        while (!QFileInfo::exists(probe) && QFileInfo(probe).dir().path() != probe)
            probe = QFileInfo(probe).dir().path();
        const QStorageInfo vol(probe);
        const qint64 need = requiredBytes(payloadDir());
        const bool existing = QFileInfo::exists(QDir(dest).filePath(QStringLiteral("install.json")));
        if (QFileInfo(dest).exists() && !QFileInfo(dest).isDir()) {
            m_space->setText(QStringLiteral("That path is a file."));
        } else if (vol.isValid() && vol.bytesAvailable() < need) {
            m_space->setText(QStringLiteral("Not enough free space: %1 needed, %2 free.").arg(humanBytes(need), humanBytes(vol.bytesAvailable())));
        } else {
            m_ok = true;
            m_space->setText(QStringLiteral("%1 needed, %2 free.%3")
                                 .arg(humanBytes(need), vol.isValid() ? humanBytes(vol.bytesAvailable()) : QStringLiteral("?"),
                                      existing ? QStringLiteral(" An existing installation there will be replaced.") : QString()));
        }
    }
    emit completeChanged();
}

bool DestinationSelectPage::isComplete() const
{
    return m_ok;
}

bool DestinationSelectPage::validatePage()
{
    const QString dest = QDir::cleanPath(QDir::fromNativeSeparators(m_path->text().trimmed()));
    QDir d(dest);
    if (d.exists() && !d.isEmpty(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::Hidden)
        && !QFileInfo::exists(d.filePath(QStringLiteral("install.json")))) {
        if (QMessageBox::question(this, wizard()->windowTitle(),
                                  QStringLiteral("%1 already has files in it. Install into it anyway?").arg(QDir::toNativeSeparators(dest)))
            != QMessageBox::Yes)
            return false;
    }
    m_wizard->destination = dest;
    m_wizard->desktopShortcut = m_desktop->isChecked();
    m_wizard->menuShortcut = m_menu->isChecked();
    return true;
}

InstallProgressPage::InstallProgressPage(InstallerWizard *w) : m_wizard(w)
{
    setTitle(QStringLiteral("Installing"));
    setSubTitle(QStringLiteral("The game's files are read from your disc, checked and installed."));
    auto *l = new QVBoxLayout(this);
    m_step = new QLabel;
    m_bar = new QProgressBar;
    m_bar->setRange(0, 1000);
    m_log = new QPlainTextEdit;
    m_log->setReadOnly(true);
    l->addWidget(m_step);
    l->addWidget(m_bar);
    l->addWidget(m_log);
    m_install = new Installation(this);
    connect(m_install, &Installation::step, m_step, &QLabel::setText);
    connect(m_install, &Installation::progress, this, [this](double f) { m_bar->setValue(int(f * 1000)); });
    connect(m_install, &Installation::log, m_log, &QPlainTextEdit::appendPlainText);
    connect(m_install, &Installation::finished, this, [this](bool ok, const QString &message) {
        m_ok = ok;
        if (ok) {
            QString err;
            if ((m_wizard->desktopShortcut || m_wizard->menuShortcut)
                && !createShortcuts(m_wizard->destination, m_wizard->desktopShortcut, m_wizard->menuShortcut, &err))
                m_log->appendPlainText(err);
            m_step->setText(QStringLiteral("Installed."));
            m_log->appendPlainText(QStringLiteral("Installed to %1").arg(QDir::toNativeSeparators(m_wizard->destination)));
            emit completeChanged();
            m_wizard->next();
        } else {
            m_step->setText(QStringLiteral("The installation did not finish."));
            m_log->appendPlainText(message);
            QMessageBox::warning(this, m_wizard->windowTitle(), message);
            m_wizard->button(QWizard::CancelButton)->setText(QStringLiteral("Close"));
        }
    });
}

void InstallProgressPage::initializePage()
{
    m_ok = false;
    m_log->clear();
    m_bar->setValue(0);
    m_install->start(m_wizard->source, m_wizard->destination);
}

bool InstallProgressPage::isComplete() const
{
    return m_ok;
}

bool InstallProgressPage::busy() const
{
    return m_install->running();
}

void InstallProgressPage::cancelInstall()
{
    m_step->setText(QStringLiteral("Stopping…"));
    m_install->cancel();
}

FinishedPage::FinishedPage(InstallerWizard *w) : m_wizard(w)
{
    setTitle(QStringLiteral("Ready to play"));
    auto *l = new QVBoxLayout(this);
    m_text = new QLabel;
    m_text->setWordWrap(true);
    m_launch = new QCheckBox(QStringLiteral("Start the game now"));
    m_launch->setChecked(true);
    l->addWidget(m_text);
    l->addSpacing(12);
    l->addWidget(m_launch);
    l->addStretch();
}

void FinishedPage::initializePage()
{
    m_text->setText(QStringLiteral("<p>Gauntlet: Dark Legacy is installed in <b>%1</b>.</p>"
                                   "<p>Start it from the shortcut, or run <i>%2</i> in that folder. README.txt there explains the "
                                   "controls, the settings (F10) and how to uninstall.</p>")
                        .arg(QDir::toNativeSeparators(m_wizard->destination).toHtmlEscaped(),
                             QFileInfo(gameExecutable(m_wizard->destination)).fileName()));
}

bool FinishedPage::validatePage()
{
    if (m_launch->isChecked())
        QProcess::startDetached(gameExecutable(m_wizard->destination), {}, m_wizard->destination);
    return true;
}
