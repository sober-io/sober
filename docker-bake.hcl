variable "REGISTRY" {
  default = "ghcr.io/sober-io/sober"
}

variable "TAG" {
  default = "latest"
}

group "default" {
  targets = ["sober-api", "sober-agent", "sober-scheduler", "sober-web", "sober-cli"]
}

target "_common" {
  dockerfile = "infra/docker/Dockerfile.ci"
  context    = "."
  platforms  = ["linux/amd64"]
  cache-from = ["type=gha"]
  cache-to   = ["type=gha,mode=max"]
}

target "sober-api" {
  inherits = ["_common"]
  target   = "sober-api"
  tags     = ["${REGISTRY}/sober-api:${TAG}"]
}

target "sober-agent" {
  inherits = ["_common"]
  target   = "sober-agent"
  tags     = ["${REGISTRY}/sober-agent:${TAG}"]
}

target "sober-scheduler" {
  inherits = ["_common"]
  target   = "sober-scheduler"
  tags     = ["${REGISTRY}/sober-scheduler:${TAG}"]
}

target "sober-web" {
  inherits = ["_common"]
  target   = "sober-web"
  tags     = ["${REGISTRY}/sober-web:${TAG}"]
}

target "sober-cli" {
  inherits = ["_common"]
  target   = "sober-cli"
  tags     = ["${REGISTRY}/sober-cli:${TAG}"]
}
