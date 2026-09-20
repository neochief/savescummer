#pragma once
#include "service.h"
QJsonObject demoState();
class DemoService final : public Service {
    Q_OBJECT
  public:
    using Service::Service;
    void start() override;
    void request(const QJsonObject &command, Callback callback = {}) override;

  private:
    void publish();
    QJsonObject state_ = demoState();
};
