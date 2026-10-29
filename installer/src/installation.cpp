#include "installation.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QStandardPaths>
#include <QStorageInfo>
#include <QThread>

#include <algorithm>

#ifdef Q_OS_WIN
#include <windows.h>
#endif

QString exeName(const QString &base)
{
#ifdef Q_OS_WIN
    return base + QStringLiteral(".exe");
#else
    return base;
#endif
}

QString payloadDir()
{
    return QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("payload"));
}

QString gameExecutable(const QString &dest)
{
    return QDir(dest).filePath(exeName(QStringLiteral("gdl_play")));
}

static qint64 treeSize(const QString &dir)
{
    qint64 n = 0;
    QDirIterator it(dir, QDir::Files | QDir::Hidden, QDirIterator::Subdirectories);
    while (it.hasNext()) {
        it.next();
        n += it.fileInfo().size();
    }
    return n;
}

qint64 requiredBytes(const QString &payload)
{
    // The imported data of the supported disc: 779 MiB measured
    // (docs/research/release.md), with a margin.
    return treeSize(payload) + qint64(850) * 1024 * 1024;
}

// A root holding a PS2 game: SYSTEM.CNF at the top (any case, ";1"
// versions as some mounts show them).
static bool hasSystemCnf(const QString &root)
{
    const auto names = QDir(root).entryList(QDir::Files);
    for (const QString &n : names) {
        if (n.section(QLatin1Char(';'), 0, 0).compare(QStringLiteral("SYSTEM.CNF"), Qt::CaseInsensitive) == 0)
            return true;
    }
    return false;
}

QList<DiscCandidate> discCandidates()
{
    QList<DiscCandidate> out;
    QStringList mountedDevices;
    for (const QStorageInfo &v : QStorageInfo::mountedVolumes()) {
        if (!v.isValid() || !v.isReady())
            continue;
        const QString fs = QString::fromLatin1(v.fileSystemType()).toLower();
        const bool optical = fs == QLatin1String("iso9660") || fs == QLatin1String("udf") || fs == QLatin1String("cdfs")
            || fs == QLatin1String("cd9660");
        if (!optical && !hasSystemCnf(v.rootPath()))
            continue;
        mountedDevices << QString::fromLocal8Bit(v.device());
        QString name = v.displayName();
        if (name.isEmpty())
            name = v.rootPath();
        out.append({QStringLiteral("%1  (%2)").arg(name, QDir::toNativeSeparators(v.rootPath())), v.rootPath()});
    }
#ifdef Q_OS_LINUX
    // Optical drives the desktop has not mounted: read as an image.
    const auto drives = QDir(QStringLiteral("/dev")).entryList({QStringLiteral("sr*")}, QDir::System);
    for (const QString &d : drives) {
        const QString dev = QStringLiteral("/dev/") + d;
        if (mountedDevices.contains(dev))
            continue;
        out.append({QStringLiteral("Optical drive %1 (not mounted)").arg(dev), dev});
    }
#endif
    return out;
}

static QProcess *importer(QObject *parent, const QString &program)
{
    auto *p = new QProcess(parent);
    p->setProgram(program);
    p->setProcessChannelMode(QProcess::SeparateChannels);
#ifdef Q_OS_WIN
    // A console program started by a GUI program: no console window.
    p->setCreateProcessArgumentsModifier([](QProcess::CreateProcessArguments *args) { args->flags |= CREATE_NO_WINDOW; });
#endif
    return p;
}

Probe::Probe(QObject *parent) : QObject(parent) {}

bool Probe::running() const
{
    return m_proc && m_proc->state() != QProcess::NotRunning;
}

void Probe::start(const QString &source)
{
    if (m_proc) {
        m_proc->disconnect(this);
        m_proc->kill();
        m_proc->deleteLater();
    }
    m_source = source;
    const QString payload = payloadDir();
    m_proc = importer(this, QDir(payload).filePath(exeName(QStringLiteral("gdl_import"))));
    m_proc->setArguments({QStringLiteral("probe"), source, QStringLiteral("--reference"), QDir(payload).filePath(QStringLiteral("import"))});
    QProcess *p = m_proc;
    connect(p, &QProcess::finished, this, [this, p](int, QProcess::ExitStatus) {
        if (p != m_proc)
            return;
        bool ok = false;
        QString message = QStringLiteral("The disc could not be checked.");
        const auto lines = p->readAllStandardOutput().split('\n');
        for (const QByteArray &l : lines) {
            const QJsonObject o = QJsonDocument::fromJson(l).object();
            if (o.value(QStringLiteral("event")).toString() == QLatin1String("probe")) {
                ok = o.value(QStringLiteral("supported")).toBool();
                message = o.value(QStringLiteral("message")).toString();
            } else if (o.value(QStringLiteral("event")).toString() == QLatin1String("error")) {
                message = o.value(QStringLiteral("message")).toString();
            }
        }
        emit finished(m_source, ok, message);
    });
    connect(p, &QProcess::errorOccurred, this, [this, p](QProcess::ProcessError e) {
        if (p == m_proc && e == QProcess::FailedToStart)
            emit finished(m_source, false, QStringLiteral("The importer (%1) could not be started.").arg(p->program()));
    });
    p->start();
}

Installation::Installation(QObject *parent) : QObject(parent) {}

Installation::~Installation()
{
    m_cancel.storeRelaxed(1);
    if (m_copier) {
        m_copier->wait();
    }
    if (m_proc && m_proc->state() != QProcess::NotRunning) {
        m_proc->kill();
        m_proc->waitForFinished(5000);
    }
}

void cleanLeftovers(const QString &dest)
{
    const auto entries = QDir(dest).entryList({QStringLiteral(".gdl-import-*"), QStringLiteral(".gdl-old-*")}, QDir::Dirs | QDir::Hidden);
    for (const QString &e : entries)
        QDir(QDir(dest).filePath(e)).removeRecursively();
}

void Installation::start(const QString &source, const QString &dest)
{
    m_source = source;
    m_dest = dest;
    m_running = true;
    m_done = false;
    m_error.clear();
    m_cancel.storeRelaxed(0);
    m_fresh = !QFileInfo::exists(QDir(dest).filePath(QStringLiteral("install.json")));
    m_copied.clear();
    emit step(QStringLiteral("Copying the game's programs"));
    const QString payload = payloadDir();
    const qint64 total = treeSize(payload);
    // The payload is copied on a worker thread; the page stays responsive.
    m_copier = QThread::create([this, payload, dest, total]() {
        if (!QDir().mkpath(dest)) {
            m_error = QStringLiteral("The folder %1 could not be created.").arg(QDir::toNativeSeparators(dest));
            return;
        }
        qint64 done = 0;
        QDirIterator it(payload, QDir::Files | QDir::Hidden, QDirIterator::Subdirectories);
        while (it.hasNext()) {
            if (m_cancel.loadRelaxed()) {
                m_error = QStringLiteral("Cancelled.");
                return;
            }
            const QString from = it.next();
            const QString rel = QDir(payload).relativeFilePath(from);
            const QString to = QDir(dest).filePath(rel);
            QDir().mkpath(QFileInfo(to).absolutePath());
            QFile::remove(to);
            if (!QFile::copy(from, to)) {
                m_error = QStringLiteral("%1 could not be copied (is the folder writable, and is there space?).").arg(rel);
                return;
            }
            m_copied << to;
            done += QFileInfo(from).size();
            QMetaObject::invokeMethod(this, [this, done, total]() { emit progress(total ? 0.05 * done / total : 0.05); }, Qt::QueuedConnection);
        }
    });
    connect(m_copier, &QThread::finished, this, [this]() {
        m_copier->deleteLater();
        m_copier = nullptr;
        if (!m_error.isEmpty()) {
            finish(false, m_error);
            return;
        }
        runImporter();
    });
    m_copier->start();
}

void Installation::runImporter()
{
    emit step(QStringLiteral("Importing the game from your disc"));
    m_proc = importer(this, QDir(m_dest).filePath(exeName(QStringLiteral("gdl_import"))));
    m_proc->setArguments({QStringLiteral("install"), m_source, m_dest, QStringLiteral("--reference"), QDir(m_dest).filePath(QStringLiteral("import"))});
    connect(m_proc, &QProcess::readyReadStandardOutput, this, [this]() {
        m_buffer += m_proc->readAllStandardOutput();
        int nl;
        while ((nl = m_buffer.indexOf('\n')) >= 0) {
            handleLine(m_buffer.left(nl));
            m_buffer.remove(0, nl + 1);
        }
    });
    connect(m_proc, &QProcess::readyReadStandardError, this, [this]() {
        const QString text = QString::fromLocal8Bit(m_proc->readAllStandardError()).trimmed();
        if (!text.isEmpty())
            emit log(text);
    });
    connect(m_proc, &QProcess::finished, this, [this](int code, QProcess::ExitStatus status) {
        if (!m_buffer.isEmpty())
            handleLine(m_buffer);
        m_buffer.clear();
        if (m_cancel.loadRelaxed()) {
            cleanLeftovers(m_dest);
            finish(false, QStringLiteral("Cancelled."));
        } else if (status != QProcess::NormalExit || code != 0 || !m_done) {
            cleanLeftovers(m_dest);
            finish(false, m_error.isEmpty() ? QStringLiteral("The import stopped unexpectedly (exit code %1).").arg(code) : m_error);
        } else {
            QFile marker(QDir(m_dest).filePath(QStringLiteral("install.json")));
            if (marker.open(QIODevice::WriteOnly)) {
                QJsonObject o{{QStringLiteral("installed"), QDateTime::currentDateTimeUtc().toString(Qt::ISODate)},
                              {QStringLiteral("source"), QDir::toNativeSeparators(m_source)},
                              {QStringLiteral("installer"), QStringLiteral(GDL_INSTALLER_VERSION)}};
                marker.write(QJsonDocument(o).toJson());
            }
            finish(true, QString());
        }
    });
    connect(m_proc, &QProcess::errorOccurred, this, [this](QProcess::ProcessError e) {
        if (e == QProcess::FailedToStart)
            finish(false, QStringLiteral("The importer could not be started."));
    });
    m_proc->start();
}

void Installation::handleLine(const QByteArray &line)
{
    const QJsonObject o = QJsonDocument::fromJson(line).object();
    const QString event = o.value(QStringLiteral("event")).toString();
    if (event == QLatin1String("progress")) {
        emit progress(0.05 + 0.95 * o.value(QStringLiteral("overall")).toDouble());
    } else if (event == QLatin1String("step")) {
        emit step(o.value(QStringLiteral("title")).toString());
        emit log(o.value(QStringLiteral("title")).toString());
    } else if (event == QLatin1String("log")) {
        emit log(o.value(QStringLiteral("message")).toString());
    } else if (event == QLatin1String("error")) {
        m_error = o.value(QStringLiteral("message")).toString();
        emit log(QStringLiteral("Error: ") + m_error);
    } else if (event == QLatin1String("done")) {
        m_done = true;
        emit progress(1.0);
    }
}

void Installation::cancel()
{
    m_cancel.storeRelaxed(1);
    if (m_proc && m_proc->state() != QProcess::NotRunning)
        m_proc->kill();
}

void Installation::finish(bool ok, const QString &message)
{
    if (!m_running)
        return;
    m_running = false;
    if (!ok && m_fresh) {
        // An unfinished first installation leaves nothing behind.
        cleanLeftovers(m_dest);
        for (const QString &f : std::as_const(m_copied))
            QFile::remove(f);
        QDirIterator dirs(m_dest, QDir::Dirs | QDir::NoDotAndDotDot | QDir::Hidden, QDirIterator::Subdirectories);
        QStringList found;
        while (dirs.hasNext())
            found << dirs.next();
        std::sort(found.begin(), found.end(), [](const QString &a, const QString &b) { return a.size() > b.size(); });
        for (const QString &d : std::as_const(found))
            QDir().rmdir(d);
        QDir().rmdir(m_dest);
    }
    emit finished(ok, message);
}

bool createShortcuts(const QString &dest, bool desktop, bool menu, QString *error)
{
    const QString exe = gameExecutable(dest);
    QStringList targets;
#ifdef Q_OS_WIN
    if (menu)
        targets << QDir(QStandardPaths::writableLocation(QStandardPaths::ApplicationsLocation)).filePath(QStringLiteral("Gauntlet Dark Legacy.lnk"));
    if (desktop)
        targets << QDir(QStandardPaths::writableLocation(QStandardPaths::DesktopLocation)).filePath(QStringLiteral("Gauntlet Dark Legacy.lnk"));
    for (const QString &t : targets) {
        QDir().mkpath(QFileInfo(t).absolutePath());
        QFile::remove(t);
        // QFile::link makes a .lnk shortcut on Windows.
        if (!QFile::link(exe, t)) {
            if (error)
                *error = QStringLiteral("The shortcut %1 could not be created.").arg(QDir::toNativeSeparators(t));
            return false;
        }
    }
#else
    QString entry = QStringLiteral("[Desktop Entry]\nType=Application\nName=Gauntlet: Dark Legacy\n"
                                   "Comment=Native port, played with the data of your own PS2 disc\n"
                                   "Exec=\"%1\"\nPath=%2\nTerminal=false\nCategories=Game;ActionGame;\n")
                        .arg(exe, dest);
    const QString icon = QDir(dest).filePath(QStringLiteral("icon.png"));
    if (QFileInfo::exists(icon))
        entry += QStringLiteral("Icon=%1\n").arg(icon);
    if (menu)
        targets << QDir(QStandardPaths::writableLocation(QStandardPaths::ApplicationsLocation)).filePath(QStringLiteral("gauntlet-dark-legacy.desktop"));
    if (desktop)
        targets << QDir(QStandardPaths::writableLocation(QStandardPaths::DesktopLocation)).filePath(QStringLiteral("gauntlet-dark-legacy.desktop"));
    for (const QString &t : targets) {
        QDir().mkpath(QFileInfo(t).absolutePath());
        QFile f(t);
        if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate) || f.write(entry.toUtf8()) < 0) {
            if (error)
                *error = QStringLiteral("The shortcut %1 could not be created.").arg(t);
            return false;
        }
        f.close();
        f.setPermissions(f.permissions() | QFileDevice::ExeOwner | QFileDevice::ExeUser);
    }
#endif
    return true;
}
